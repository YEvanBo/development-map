use development_map::{
    identity::Binding,
    mcp::Session,
    store::{MapStore, NodeInput},
};
use serde_json::{json, Value};
use std::{
    io::Write,
    process::{Command, Stdio},
};
const TOKEN: &str = "mcp-isolated-worker-token-1234567890";
const READER: &str = "mcp-isolated-reader-token-1234567890";
fn setup() -> (tempfile::TempDir, MapStore) {
    let d = tempfile::tempdir().unwrap();
    let s = MapStore::new(d.path().join("map.sqlite"));
    s.migrate().unwrap();
    for project in ["p", "other"] {
        s.project(project, project, "test").unwrap();
        s.adopt_architecture(project, "v1", "design", "1234567890123456")
            .unwrap();
    }
    s.node(&NodeInput {
        id: "node".into(),
        project_id: "p".into(),
        parent_id: None,
        kind: "module".into(),
        title: "node".into(),
        goal: "recover".into(),
        sort_key: "1".into(),
        plan_state: "active".into(),
        owner: Some("EvanBo".into()),
        generation: 1,
        architecture_ref: None,
    })
    .unwrap();
    for (token, role) in [(TOKEN, "worker"), (READER, "reader")] {
        s.bind_identity(
            token,
            &Binding {
                subject: "test".into(),
                project_id: "p".into(),
                task_id: "test".into(),
                owner: "EvanBo".into(),
                role: role.into(),
                node_id: if role == "worker" {
                    Some("node".into())
                } else {
                    None
                },
                generation: if role == "worker" { Some(1) } else { None },
                source_ref: "fixture".into(),
                expires_at: 4102444800,
            },
        )
        .unwrap();
    }
    (d, s)
}
fn init() -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"isolated-client","version":"1"}}})
}
fn ready(s: &MapStore, token: &str) -> Session {
    let mut x = Session::default();
    assert_eq!(
        x.handle(s, token, init()).unwrap()["result"]["protocolVersion"],
        "2025-06-18"
    );
    assert!(x
        .handle(
            s,
            token,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .is_none());
    x
}
fn call(name: &str, args: Value) -> Value {
    json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":args}})
}
#[test]
fn lifecycle_negotiation_and_no_execution_tools() {
    let (_d, s) = setup();
    let mut x = Session::default();
    assert_eq!(
        x.handle(
            &s,
            TOKEN,
            json!({"jsonrpc":"2.0","id":0,"method":"tools/list"})
        )
        .unwrap()["error"]["code"],
        -32000
    );
    assert_eq!(
        x.handle(
            &s,
            TOKEN,
            json!({"jsonrpc":"2.0","id":0,"method":"server/discover"})
        )
        .unwrap()["error"]["code"],
        -32601
    );
    let mut x = ready(&s, TOKEN);
    let list = x
        .handle(
            &s,
            TOKEN,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}),
        )
        .unwrap();
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 3);
    assert_eq!(
        x.handle(&s, TOKEN, init()).unwrap()["error"]["code"],
        -32600
    );
}
#[test]
fn project_and_worker_scopes_are_preserved() {
    let (_d, s) = setup();
    let mut w = ready(&s, TOKEN);
    let mut r = ready(&s, READER);
    assert_eq!(
        r.handle(&s, READER, call("map_project", json!({"project_id":"p"})))
            .unwrap()["result"]["isError"],
        false
    );
    for token in [TOKEN, READER] {
        assert_eq!(
            r.handle(
                &s,
                token,
                call("map_project", json!({"project_id":"other"}))
            )
            .unwrap()["result"]["isError"],
            true
        );
    }
    assert_eq!(
        w.handle(&s, TOKEN, call("map_project", json!({"project_id":"p"})))
            .unwrap()["result"]["isError"],
        true
    );
    assert_eq!(
        w.handle(
            &s,
            TOKEN,
            call("map_resume", json!({"project_id":"p","node_id":"node"}))
        )
        .unwrap()["result"]["isError"],
        false
    );
}
#[test]
fn stdio_persists_and_retries_without_stdout_secrets() {
    let (d, s) = setup();
    let bundle = json!({"submission_key":"mcp-v1","checkpoint":{"node_id":"node","owner":"EvanBo","generation":1,"version":1,"digest":"1234567890123456","summary":"real MCP write","source_ref":"test"},"artifacts":[]});
    let mut c = Command::new(env!("CARGO_BIN_EXE_map-mcp"))
        .arg(d.path().join("map.sqlite"))
        .env("MAP_AGENT_TOKEN", TOKEN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = c.stdin.take().unwrap();
    for q in [
        init(),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        call("map_submit", json!({"submission":bundle})),
        call("map_submit", json!({"submission":bundle})),
    ] {
        writeln!(input, "{q}").unwrap();
    }
    drop(input);
    let out = c.wait_with_output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(!text.contains(TOKEN));
    let lines: Vec<Value> = text
        .lines()
        .map(|v| serde_json::from_str(v).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    let a: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let b: Value =
        serde_json::from_str(lines[2]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(a["created"], true);
    assert_eq!(b["created"], false);
    assert_eq!(a["event_id"], b["event_id"]);
    s.read(|tx| {
        let count: i64 = tx.query_row("SELECT count(*) FROM map_checkpoint_index", [], |r| {
            r.get(0)
        })?;
        assert_eq!(count, 1);
        Ok(())
    })
    .unwrap();
}
#[test]
fn malformed_unknown_and_oversize_inputs_are_contained() {
    let (d, s) = setup();
    let mut x = ready(&s, TOKEN);
    let v = x
        .handle(
            &s,
            TOKEN,
            call("map_resume", json!({"project_id":"p","progress":{}})),
        )
        .unwrap();
    assert_eq!(v["result"]["isError"], true);
    let mut c = Command::new(env!("CARGO_BIN_EXE_map-mcp"))
        .arg(d.path().join("map.sqlite"))
        .env("MAP_AGENT_TOKEN", TOKEN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = c.stdin.take().unwrap();
    input.write_all(b"invalid-json\n").unwrap();
    let bytes = vec![b'x'; 131073];
    let _ = input.write_all(&bytes);
    drop(input);
    let out = c.wait_with_output().unwrap();
    assert!(!out.status.success());
    let response: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(response["error"]["code"], -32700);
    assert!(!String::from_utf8(out.stderr).unwrap().contains(TOKEN));
}
#[cfg(unix)]
#[test]
fn world_readable_token_file_is_rejected() {
    use std::os::unix::fs::PermissionsExt;
    let (d, _s) = setup();
    let p = d.path().join("credential");
    std::fs::write(&p, TOKEN).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_map-mcp"))
        .arg(d.path().join("map.sqlite"))
        .arg("--token-file")
        .arg(&p)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
}

#[test]
fn schema_mismatch_rejects_reads_and_writes_without_side_effects() {
    let (_d, s) = setup();
    let bundle = json!({"submission_key":"mcp-v1","checkpoint":{"node_id":"node","owner":"EvanBo","generation":1,"version":1,"digest":"1234567890123456","summary":"real MCP write","source_ref":"test"},"artifacts":[]});
    let db = s.connect(false).unwrap();
    let counts = || {
        db.query_row("SELECT (SELECT count(*) FROM map_checkpoint_index),(SELECT count(*) FROM map_submissions),(SELECT count(*) FROM map_events)",[],|r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?))).unwrap()
    };
    let baseline = counts();
    let mut session = ready(&s, TOKEN);
    for (version, reason) in [(9, "migration required"), (11, "upgrade map tools")] {
        db.execute("UPDATE map_schema_migrations SET version=?", [version])
            .unwrap();
        for request in [
            call("map_resume", json!({"project_id":"p","node_id":"node"})),
            call("map_submit", json!({"submission":bundle})),
        ] {
            let reply = session.handle(&s, TOKEN, request).unwrap();
            assert_eq!(reply["result"]["isError"], true);
            assert!(reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains(reason));
        }
        assert_eq!(counts(), baseline);
    }
}

#[test]
fn concurrent_processes_recover_pending_file_after_stale_base_exit() {
    use std::io::{BufRead, BufReader};
    let (dir, store) = setup();
    let launch = || {
        let mut child = Command::new(env!("CARGO_BIN_EXE_map-mcp"))
            .arg(dir.path().join("map.sqlite"))
            .env("MAP_AGENT_TOKEN", TOKEN)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        writeln!(input, "{}", init()).unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        assert!(line.contains("protocolVersion"));
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        let request = move |name: &str, args: Value| {
            writeln!(input, "{}", call(name, args)).unwrap();
            line.clear();
            output.read_line(&mut line).unwrap();
            let response: Value = serde_json::from_str(&line).unwrap();
            let body = response["result"]["content"][0]["text"].as_str().unwrap();
            if response["result"]["isError"] == true {
                Err(body.to_owned())
            } else {
                Ok(serde_json::from_str::<Value>(body).unwrap())
            }
        };
        (child, request)
    };
    let resume = json!({"project_id":"p","node_id":"node"});
    let (mut first, mut a) = launch();
    let (mut second, mut b) = launch();
    assert_eq!(
        a("map_resume", resume.clone()).unwrap()["checkpoint"],
        Value::Null
    );
    assert_eq!(
        b("map_resume", resume.clone()).unwrap()["checkpoint"],
        Value::Null
    );
    let root = dir.path();
    let artifact = |source: &str, content: &str, key: &str, base: i64, version: i64| {
        use sha2::{Digest, Sha256};
        std::fs::write(root.join(source), content).unwrap();
        json!({"submission":{"base_checkpoint_rowid":base,"submission_key":key,"artifacts":[],
            "checkpoint":{"node_id":"node","owner":"EvanBo","generation":1,"version":version,
            "digest":format!("{:x}",Sha256::digest(content.as_bytes())),"summary":content,"source_ref":source}}})
    };
    let original = artifact("a.txt", "A delivered\n", "A", 0, 1);
    let pending = artifact("b.txt", "B pending\n", "B", 0, 3);
    let session_dir = root.join(".devmap-pending/B");
    std::fs::create_dir_all(&session_dir).unwrap();
    let handoff = session_dir.join("pending-B.json");
    std::fs::write(&handoff, serde_json::to_vec(&pending).unwrap()).unwrap();
    let rejected = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/integrations/skills/development-map/scripts/save-pending.py"
        ))
        .args([".devmap-pending/B", "a.txt", "b.txt"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    let snapshot = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/integrations/skills/development-map/scripts/save-pending.py"
        ))
        .args([
            ".devmap-pending/B",
            ".devmap-pending/B/pending-B.json",
            "b.txt",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(snapshot.status.success());
    let snapshot = String::from_utf8(snapshot.stdout).unwrap();
    let receipt = a("map_submit", original.clone()).unwrap();
    let counts = || {
        store.read(|tx| Ok(tx.query_row(
        "SELECT (SELECT count(*) FROM map_checkpoint_index),(SELECT count(*) FROM map_submissions),(SELECT count(*) FROM map_events)",
        [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?)).unwrap()
    };
    let before_conflict = counts();
    assert!(b("map_submit", pending)
        .unwrap_err()
        .contains("stale checkpoint base"));
    assert_eq!(counts(), before_conflict);
    drop(a);
    drop(b);
    assert!(first.wait().unwrap().success() && second.wait().unwrap().success());
    let (mut restarted, mut client) = launch();
    let head = client("map_resume", resume.clone()).unwrap()["checkpoint"].clone();
    assert_eq!(head["rowid"], receipt["checkpoint_rowid"]);
    std::fs::remove_file(&handoff).unwrap();
    std::fs::remove_file(root.join("b.txt")).unwrap();
    let restored = root.join("restored");
    let status = Command::new("python3")
        .args(["-m", "zipfile", "-e"])
        .arg(root.join(snapshot.trim()))
        .arg(&restored)
        .status()
        .unwrap();
    assert!(status.success());
    let saved: Value =
        serde_json::from_slice(&std::fs::read(restored.join("handoff")).unwrap()).unwrap();
    let local = std::fs::read_to_string(
        restored.join("files").join(
            saved["submission"]["checkpoint"]["source_ref"]
                .as_str()
                .unwrap(),
        ),
    )
    .unwrap();
    let read_source = |cp: &Value| {
        std::fs::read_to_string(root.join(cp["source_ref"].as_str().unwrap())).unwrap()
    };
    let combined = read_source(&head) + &local;
    assert_eq!(combined, "A delivered\nB pending\n");
    let final_request = artifact(
        "combined.txt",
        &combined,
        "B-merged",
        head["rowid"].as_i64().unwrap(),
        2,
    );
    assert_eq!(
        client("map_submit", final_request).unwrap()["created"],
        true
    );
    assert_eq!(client("map_submit", original).unwrap()["created"], false);
    assert_eq!(
        read_source(&client("map_resume", resume).unwrap()["checkpoint"]),
        combined
    );
    drop(client);
    assert!(restarted.wait().unwrap().success());
    store
        .read(|tx| {
            assert_eq!(
                tx.query_row("SELECT count(*) FROM map_checkpoint_index", [], |r| r
                    .get::<_, i64>(0))?,
                2
            );
            Ok(())
        })
        .unwrap();
}
