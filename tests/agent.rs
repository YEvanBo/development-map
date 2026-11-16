use development_map::{
    agent::Resume,
    identity::Binding,
    store::{CheckpointInput, MapStore, NodeInput},
    submission::Submission,
};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};
const TOKEN: &str = "agent-test-worker-token-1234567890";
fn setup() -> (tempfile::TempDir, MapStore) {
    let tmp = tempfile::tempdir().unwrap();
    let s = MapStore::new(tmp.path().join("map.sqlite"));
    s.migrate().unwrap();
    for p in ["p", "other"] {
        s.project(p, p, "fixture").unwrap();
        s.adopt_architecture(p, "v1", "fixture", "1234567890123456")
            .unwrap();
        s.node(&NodeInput {
            id: format!("{p}/module"),
            project_id: p.into(),
            parent_id: None,
            kind: "module".into(),
            title: "Module".into(),
            goal: "Recover a result".into(),
            sort_key: "1".into(),
            plan_state: "active".into(),
            owner: Some("owner".into()),
            generation: 2,
            architecture_ref: None,
        })
        .unwrap();
    }
    s.bind_identity(TOKEN, &binding("worker", 4102444800))
        .unwrap();
    (tmp, s)
}
fn binding(role: &str, expires_at: i64) -> Binding {
    Binding {
        subject: "account/agent".into(),
        project_id: "p".into(),
        task_id: "stable-task".into(),
        owner: "owner".into(),
        role: role.into(),
        node_id: if role == "worker" {
            Some("p/module".into())
        } else {
            None
        },
        generation: if role == "worker" { Some(2) } else { None },
        source_ref: "trusted-host-assignment".into(),
        expires_at,
    }
}
fn query(v: Value) -> Resume {
    serde_json::from_value(v).unwrap()
}
fn bundle() -> Submission {
    Submission {
        base_checkpoint_rowid: None,
        submission_key: "p-module-g2-v1".into(),
        checkpoint: CheckpointInput {
            node_id: "p/module".into(),
            owner: "owner".into(),
            generation: 2,
            version: 1,
            digest: "1234567890123456".into(),
            summary: "Verified isolated host result".into(),
            source_ref: "module/CHECKPOINT.md".into(),
            git_revision: None,
        },
        artifacts: vec![],
        resources: None,
    }
}
#[test]
fn identity_recovery_and_atomic_idempotent_submission() {
    let (_tmp, s) = setup();
    let q = query(json!({"project_id":"p","task_id":"stable-task"}));
    let before = s.agent_resume(TOKEN, &q).unwrap();
    assert_eq!(before["node"]["id"], "p/module");
    assert_eq!(before["capabilities"]["submit"], true);
    assert!(before["checkpoint"].is_null());
    for v in [
        json!({"project_id":"other","node_id":"other/module"}),
        json!({"project_id":"p","node_id":"other/module"}),
        json!({"project_id":"p","task_id":"invented"}),
        json!({"project_id":"p","task_id":"stable-task","node_id":"p/module"}),
    ] {
        assert!(s.agent_resume(TOKEN, &query(v)).is_err());
    }
    let b = bundle();
    let a = s.submit_bound(&b, Some(TOKEN)).unwrap();
    let replay = s.submit_bound(&b, Some(TOKEN)).unwrap();
    assert!(a.created);
    assert!(!replay.created);
    assert_eq!(a.event_id, replay.event_id);
    let mut changed = b.clone();
    changed.checkpoint.summary = "different".into();
    assert!(s.submit_bound(&changed, Some(TOKEN)).is_err());
    changed = b.clone();
    changed.checkpoint.owner = "forged".into();
    assert!(s.submit_bound(&changed, Some(TOKEN)).is_err());
    changed = b.clone();
    changed.checkpoint.generation = 1;
    assert!(s.submit_bound(&changed, Some(TOKEN)).is_err());
    let after = s.agent_resume(TOKEN, &q).unwrap();
    assert_eq!(after["checkpoint"]["source_ref"], "module/CHECKPOINT.md");
    assert_eq!(after["checkpoint"]["version"], 1);
    s.read(|tx| {
        assert_eq!(
            tx.query_row("SELECT COUNT(*) FROM map_checkpoint_index", [], |r| r
                .get::<_, i64>(0))?,
            1
        );
        assert_eq!(
            tx.query_row("SELECT COUNT(*) FROM map_submission_identity", [], |r| r
                .get::<_, i64>(0))?,
            1
        );
        assert_eq!(
            tx.query_row("SELECT COUNT(*) FROM map_acceptances", [], |r| r
                .get::<_, i64>(0))?,
            0
        );
        Ok(())
    })
    .unwrap();
    let read = "reader-token-for-agent-1234567890";
    s.bind_identity(read, &binding("reader", 4102444800))
        .unwrap();
    assert_eq!(
        s.agent_resume(read, &query(json!({"project_id":"p"})))
            .unwrap()["capabilities"]["submit"],
        false
    );
    assert!(s.submit_bound(&b, Some(read)).is_err());
    let expired = "expired-token-for-agent-1234567890";
    s.bind_identity(expired, &binding("reader", 1)).unwrap();
    assert!(s.agent_resume(expired, &q).is_err());
    s.revoke_identity(TOKEN).unwrap();
    assert!(s.agent_resume(TOKEN, &q).is_err());
    assert!(s.submit_bound(&b, Some(TOKEN)).is_err());
}
#[test]
fn cli_enforces_token_and_persists_result() {
    let (tmp, s) = setup();
    let db = tmp.path().join("map.sqlite");
    let call = |action: &str, input: Value, token: Option<&str>| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_map-agent"));
        c.args([action, db.to_str().unwrap()])
            .env_remove("MAP_AGENT_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(token) = token {
            c.env("MAP_AGENT_TOKEN", token);
        }
        let mut p = c.spawn().unwrap();
        p.stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        p.wait_with_output().unwrap()
    };
    assert!(!call("resume", json!({"project_id":"p"}), None)
        .status
        .success());
    assert!(call(
        "submit",
        serde_json::to_value(bundle()).unwrap(),
        Some(TOKEN)
    )
    .status
    .success());
    let r = call(
        "resume",
        json!({"project_id":"p","task_id":"stable-task"}),
        Some(TOKEN),
    );
    assert!(r.status.success(), "{:?}", r.stderr);
    let v: Value = serde_json::from_slice(&r.stdout).unwrap();
    assert_eq!(v["checkpoint"]["version"], 1);
    assert_eq!(v["capabilities"]["bridge_required"], false);
    s.revoke_identity(TOKEN).unwrap();
    assert!(!call("resume", json!({"project_id":"p"}), Some(TOKEN))
        .status
        .success());
}
