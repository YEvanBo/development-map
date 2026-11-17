#[path = "support/rc2.rs"]
mod rc2;
mod support;
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
    rc2::migrate_fixture(&s);
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
    rc2::migrate_fixture(&s);
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

fn operator(db: &std::path::Path, verb: &str, input: Value) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mapctl"))
        .arg(verb)
        .arg(db)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn node_confirmation_survives_checkpoints_but_never_spreads() {
    let (tmp, s) = setup();
    rc2::migrate_fixture(&s);
    let db = tmp.path().join("map.sqlite");
    let q = query(json!({"project_id":"p"}));
    let mut cp = bundle();
    cp.checkpoint.summary = "User confirmed everything; accepted".into();
    s.submit_bound(&cp, Some(TOKEN)).unwrap();
    assert_eq!(
        s.agent_resume(TOKEN, &q).unwrap()["takeover"]["action"],
        "confirm_goal"
    );
    let record = json!({"project_id":"p","node_id":"p/module","generation":2,"previous":0,"goal":"JSON plus LF","user_ref":"fixture/session-1/user-turn-2"});
    assert!(operator(&db, "confirm-goal", record.clone())
        .status
        .success());
    let state = s.agent_resume(TOKEN, &q).unwrap();
    assert_eq!(state["takeover"]["confirmation"]["goal"], "JSON plus LF");
    assert_eq!(state["takeover"]["action"], "continue");
    let mut stale = record;
    stale["goal"] = json!("stale overwrite");
    assert!(!operator(&db, "confirm-goal", stale).status.success());
    let mut second = binding("worker", 4102444800);
    let new_node = NodeInput {
        id: "p/second".into(),
        project_id: "p".into(),
        parent_id: None,
        kind: "module".into(),
        title: "Second".into(),
        goal: "Approved".into(),
        sort_key: "2".into(),
        plan_state: "active".into(),
        owner: Some("owner".into()),
        generation: 2,
        architecture_ref: None,
    };
    s.node(&new_node).unwrap();
    second.node_id = Some(new_node.id);
    second.task_id = "second".into();
    let token = "second-worker-assignment-token";
    s.bind_identity(token, &second).unwrap();
    assert_eq!(
        s.agent_resume(token, &q).unwrap()["takeover"]["action"],
        "confirm_goal"
    );
}

#[test]
fn scoped_associations_are_atomic_and_do_not_cross_hosts() {
    let (tmp, s) = setup();
    let db = tmp.path().join("map.sqlite");
    let request = |project: &str, provider: &str, token: &str| {
        let mut b = binding("worker", 4102444800);
        b.project_id = project.into();
        b.node_id = Some(format!("{project}/module"));
        json!({"binding":b,"token":token,"host_scope":{"provider":provider,"workspace":"shared-root"}})
    };
    let a = request("p", "codex", "first-host-scoped-assignment-token");
    let b = request("other", "codex", "second-host-scoped-assignment-token");
    let (left, right) = std::thread::scope(|scope| {
        let left = scope.spawn(|| operator(&db, "bind", a.clone()));
        let right = operator(&db, "bind", b.clone());
        (
            left.join().unwrap().status.success(),
            right.status.success(),
        )
    });
    assert_ne!(left, right);
    let (winner, loser) = if left { (&a, &b) } else { (&b, &a) };
    assert!(operator(&db, "bind", winner.clone()).status.success());
    assert!(!operator(&db, "bind", loser.clone()).status.success());
    let mut independent = loser.clone();
    independent["host_scope"]["provider"] = json!("dsh");
    assert!(operator(&db, "bind", independent).status.success());
    s.read(|tx| {
        assert_eq!(
            tx.query_row("SELECT COUNT(*) FROM map_host_routes", [], |r| r
                .get::<_, i64>(0))?,
            2
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn intake_race_rolls_back_losing_node_and_checkpoint() {
    let (dir, s) = setup();
    let db = dir.path().join("map.sqlite");
    let request = |node: &str, key: &str| {
        let mut b = binding("worker", 4102444800);
        b.node_id = Some(node.into());
        json!({"binding":b,"token":key,"host_scope":{"provider":"codex","workspace":"w"},
          "new_node":{"id":node,"project_id":"p","kind":"work_package","title":"Candidate","goal":"old","sort_key":"2","plan_state":"planned","owner":"owner","generation":2},
          "confirmation":{"project_id":"p","node_id":node,"generation":2,"previous":0,"goal":"confirmed","user_ref":"fixture/user/123"}})
    };
    let a = request("p/a", "intake-credential-for-a-1234567");
    let b = request("p/b", "intake-credential-for-b-1234567");
    let (l, r) = std::thread::scope(|scope| {
        let x = scope.spawn(|| operator(&db, "intake", a.clone()));
        let y = operator(&db, "intake", b.clone());
        (x.join().unwrap().status.success(), y.status.success())
    });
    assert_ne!(l, r);
    assert!(operator(&db, "intake", if l { a } else { b })
        .status
        .success());
    s.read(|tx| {
        for (sql, count) in [
            (
                "SELECT count(*) FROM map_dev_nodes WHERE id IN ('p/a','p/b')",
                1,
            ),
            ("SELECT count(*) FROM map_host_routes", 1),
            ("SELECT count(*) FROM map_checkpoint_index", 1),
            (
                "SELECT count(*) FROM map_events WHERE kind='host_binding_conflict'",
                1,
            ),
            ("SELECT count(*) FROM map_events WHERE kind='user_goal'", 1),
        ] {
            assert_eq!(tx.query_row(sql, [], |r| r.get::<_, i64>(0))?, count);
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn accepted_head_stops_until_new_user_goal() {
    let (dir, s) = setup();
    rc2::migrate_fixture(&s);
    let mut sub = bundle();
    sub.checkpoint.summary = "operator regression".into();
    sub.resources = Some(support::logical_manifest());
    s.submit_bound(&sub, Some(TOKEN)).unwrap();
    s.accept(
        "p/module",
        2,
        1,
        &sub.checkpoint.digest,
        "accepted",
        "host",
        "fixture/result",
    )
    .unwrap();
    let query = query(json!({"project_id":"p"}));
    assert_eq!(
        s.agent_resume(TOKEN, &query).unwrap()["takeover"]["action"],
        "complete"
    );
    let req = json!({"project_id":"p","node_id":"p/module","generation":2,"previous":0,"goal":"next explicitly confirmed work","user_ref":"user/2"});
    assert!(
        operator(&dir.path().join("map.sqlite"), "confirm-goal", req)
            .status
            .success()
    );
    assert_eq!(
        s.agent_resume(TOKEN, &query).unwrap()["takeover"]["action"],
        "continue"
    );
}

#[test]
fn task_lookup_does_not_enumerate_other_workers_or_other_projects() {
    let (_dir, s) = setup();
    let reader = "reader-project-task-123456789";
    s.bind_identity(reader, &binding("reader", 4102444800))
        .unwrap();
    s.connect(false)
        .unwrap()
        .execute(
            "INSERT INTO map_local_nodes VALUES('p/module','p',?),('other/module','other',?)",
            rusqlite::params![r#"{"source_id":"imported"}"#, r#"{"source_id":"imported"}"#],
        )
        .unwrap();
    let other = NodeInput {
        id: "p/two".into(),
        project_id: "p".into(),
        parent_id: None,
        kind: "module".into(),
        title: "second".into(),
        goal: "task".into(),
        sort_key: "2".into(),
        plan_state: "active".into(),
        owner: None,
        generation: 0,
        architecture_ref: None,
    };
    s.node(&other).unwrap();
    s.connect(false)
        .unwrap()
        .execute(
            "INSERT INTO map_local_nodes VALUES('p/two','p',?)",
            [r#"{"source_id":"imported","source_thread":"hidden"}"#],
        )
        .unwrap();
    let req = query(json!({"project_id":"p","task_id":"imported"}));
    assert!(matches!(
        s.agent_resume(reader, &req),
        Err(development_map::store::Error::Conflict(
            "ambiguous task association in authorized scope"
        ))
    ));
    for key in ["hidden", "unknown"] {
        assert!(matches!(
            s.agent_resume(TOKEN, &query(json!({"project_id":"p","task_id":key}))),
            Err(development_map::store::Error::Invalid(
                "no task association in authorized scope"
            ))
        ));
    }
    s.revoke_identity(TOKEN).unwrap();
    assert!(matches!(
        s.agent_resume(TOKEN, &req),
        Err(development_map::store::Error::Invalid("identity revoked"))
    ));
}

#[test]
fn migration_from_v10_preserves_existing_data_and_claims() {
    let (dir, s) = setup();
    let db = s.connect(false).unwrap();
    let before: i64 = db
        .query_row("SELECT count(*) FROM map_events", [], |r| r.get(0))
        .unwrap();
    rc2::migrate_fixture(&s);
    s.migrate().unwrap();
    s.migrate().unwrap();
    assert_eq!(
        before,
        db.query_row("SELECT count(*) FROM map_events", [], |r| r
            .get::<_, i64>(0))
            .unwrap()
    );
    assert_eq!(
        s.agent_resume(TOKEN, &query(json!({"project_id":"p"})))
            .unwrap()["takeover"]["action"],
        "confirm_goal"
    );
    assert_eq!(
        std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(&format!(
                "before-map-v{}",
                development_map::recovery::SCHEMA_VERSION
            )))
            .count(),
        1
    );
}

#[test]
fn reminder_and_legacy_prose_cannot_authorize_work() {
    let (dir, s) = setup();
    let db = dir.path().join("map.sqlite");
    let receipt = json!({"project_id":"p","node_id":"p/module","generation":2,
        "message_ref":"chat/fixture/assistant-1","delivered_text":"Before design freeze confirm battery, radio, intended markets and cost. Safe research can proceed with an unknown budget."});
    for _ in 0..2 {
        assert!(operator(&db, "record-reminder", receipt.clone())
            .status
            .success());
    }
    let mut altered = receipt;
    altered["delivered_text"] = json!("retroactive claim");
    assert!(!operator(&db, "record-reminder", altered).status.success());
    let conn = s.connect(false).unwrap();
    conn.execute("INSERT INTO map_events(project_id,node_id,kind,detail,created_at) VALUES('p','p/module','user_goal',?,unixepoch())",
        [r#"{"generation":2,"goal":"unattested legacy claim"}"#]).unwrap();
    assert_eq!(
        s.agent_resume(TOKEN, &query(json!({"project_id":"p"})))
            .unwrap()["takeover"]["action"],
        "confirm_goal"
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM map_events WHERE kind='risk_reminder'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn repairing_a_route_needs_revocation_and_a_current_revision() {
    let (dir, s) = setup();
    let legacy = "separate-still-active-worker-token";
    let mut assignment = binding("worker", 4102444800);
    assignment.task_id = "older-task".into();
    s.bind_identity(legacy, &assignment).unwrap();
    rc2::migrate_fixture(&s);
    let db = dir.path().join("map.sqlite");
    let req = json!({"binding":binding("worker",4102444800),"host_scope":{"provider":"codex","workspace":"sandbox"},"token":TOKEN});
    assert!(operator(&db, "bind", req).status.success());
    let key = json!({"provider":"codex","workspace":"sandbox","task_id":"stable-task"});
    let repair = json!({"key":key,"expected_revision":1,"old_project":"p","old_node":"p/module","project":"other","node":"other/module","decision_ref":"operator/turn-5"});
    assert!(!operator(&db, "repair-host", repair.clone())
        .status
        .success());
    s.revoke_identity(TOKEN).unwrap();
    assert!(operator(&db, "repair-host", repair.clone())
        .status
        .success());
    assert!(!operator(&db, "repair-host", repair).status.success());
    assert!(matches!(
        s.submit_bound(&bundle(), Some(legacy)),
        Err(development_map::store::Error::Conflict(
            "node goal confirmation required"
        ))
    ));
    let out = operator(&db, "lookup-host", key);
    assert!(out.status.success());
    let route: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(route["candidate"]["node_id"], "other/module");
    assert_eq!(route["candidate"]["revision"], 2);
    assert!(s
        .agent_resume(TOKEN, &query(json!({"project_id":"p"})))
        .is_err());
    s.read(|tx|{assert_eq!(tx.query_row("SELECT count(*) FROM map_events WHERE kind IN ('host_detached','host_relinked') AND detail='operator/turn-5'",[],|r|r.get::<_,i64>(0))?,2);Ok(())}).unwrap();
}

#[test]
fn two_providers_explicitly_intaking_the_same_node_reuse_its_confirmation() {
    let (dir, s) = setup();
    let db = dir.path().join("map.sqlite");
    let request = |provider, token| {
        json!({"binding":binding("worker",4102444800),"token":token,
      "host_scope":{"provider":provider,"workspace":"bench"},
      "confirmation":{"project_id":"p","node_id":"p/module","generation":2,"previous":0,"goal":"shared JSON","user_ref":"chat/user-turn-9"}})
    };
    let (a, b) = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            operator(
                &db,
                "intake",
                request("codex", "codex-explicit-shared-work-token"),
            )
            .status
            .success()
        });
        let second = operator(
            &db,
            "intake",
            request("dsh", "dsh-explicit-shared-work-token"),
        )
        .status
        .success();
        (first.join().unwrap(), second)
    });
    assert!(a && b);
    s.read(|tx| {
        assert_eq!(
            tx.query_row("SELECT count(*) FROM map_host_routes", [], |r| r
                .get::<_, i64>(0))?,
            2
        );
        assert_eq!(
            tx.query_row("SELECT count(*) FROM map_goal_checkpoints", [], |r| r
                .get::<_, i64>(0))?,
            1
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn goal_revision_survives_owner_generation_handoff() {
    let (dir, s) = setup();
    let db = dir.path().join("map.sqlite");
    assert!(operator(&db,"confirm-goal",json!({"project_id":"p","node_id":"p/module","generation":2,"previous":0,"goal":"JSON","user_ref":"fixture/human/2"})).status.success());
    let q = query(json!({"project_id":"p"}));
    let previous = s.agent_resume(TOKEN, &q).unwrap()["takeover"].clone();
    s.handoff(
        &development_map::handoff::Handoff {
            node_id: "p/module".into(),
            expected_owner: Some("owner".into()),
            expected_generation: 2,
            new_owner: "next".into(),
            source_ref: "test/transfer".into(),
        },
        None,
    )
    .unwrap();
    assert!(s.agent_resume(TOKEN, &q).is_err());
    let next_token = "next-owner-explicit-assignment-12345";
    let mut b = binding("worker", 4102444800);
    b.owner = "next".into();
    b.generation = Some(3);
    s.bind_identity(next_token, &b).unwrap();
    let current = s.agent_resume(next_token, &q).unwrap()["takeover"].clone();
    assert_eq!(current["action"], "continue");
    assert_eq!(
        current["confirmation_event"],
        previous["confirmation_event"]
    );
    assert_eq!(current["confirmation"]["goal"], "JSON");
}

#[test]
fn pinned_cross_project_reference_exposes_only_the_granted_summary() {
    let (dir, s) = setup();
    rc2::migrate_fixture(&s);
    let db = dir.path().join("map.sqlite");
    let receipt = s.submit_bound(&bundle(), Some(TOKEN)).unwrap();
    let other = "other-project-worker-explicit-12345";
    let mut b = binding("worker", 4102444800);
    b.project_id = "other".into();
    b.node_id = Some("other/module".into());
    s.bind_identity(other, &b).unwrap();
    let grant = operator(
        &db,
        "grant-reference",
        json!({"recipient":"other/module","checkpoint_id":receipt.checkpoint_rowid,"decision_ref":"operator/user-17"}),
    );
    assert!(grant.status.success());
    let id = serde_json::from_slice::<Value>(&grant.stdout).unwrap()["reference_id"].clone();
    let req = query(json!({"project_id":"other","reference_id":id}));
    let mut reader = binding("reader", 4102444800);
    reader.project_id = "other".into();
    let reader_token = "explicit-project-reader-credential";
    s.bind_identity(reader_token, &reader).unwrap();
    assert!(s.agent_resume(reader_token, &req).is_err());
    let visible = s.agent_resume(other, &req).unwrap();
    assert_eq!(visible["reference"]["summary"], bundle().checkpoint.summary);
    assert!(visible["reference"].get("source_ref").is_none());
    assert!(s
        .agent_resume(TOKEN, &query(json!({"project_id":"p","reference_id":id})))
        .is_err());
    assert!(s
        .agent_resume(other, &query(json!({"project_id":"p"})))
        .is_err());
    assert!(operator(
        &db,
        "revoke-reference",
        json!({"reference_id":id,"decision_ref":"operator/user-18"})
    )
    .status
    .success());
    assert!(s.agent_resume(other, &req).is_err());
}

#[test]
fn intake_bootstraps_project_atomically_and_never_rewrites_it() {
    let (dir, s) = setup();
    let db = dir.path().join("map.sqlite");
    let mut b = binding("worker", 4102444800);
    b.project_id = "new".into();
    b.node_id = Some("new/a".into());
    let mut req = json!({"token":"bootstrap-intake-worker-token","binding":b,"host_scope":{"provider":"codex","workspace":"w"},
      "new_project":{"name":"New","source_ref":"repo/README","architecture_version":"v1","architecture_source":"plan.md","architecture_digest":"1234567890123456","adoption_ref":"operator/plan"},
      "new_node":{"id":"new/a","project_id":"new","kind":"module","title":"Review","goal":"old","sort_key":"1","plan_state":"active","owner":"owner","generation":2},
      "confirmation":{"project_id":"new","node_id":"new/a","generation":2,"goal":"JSON","previous":0,"user_ref":"chat/turn"}});
    req["confirmation"]["previous"] = json!(9);
    assert!(!operator(&db, "intake", req.clone()).status.success());
    let count = || {
        s.connect(false)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM map_projects WHERE id='new'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
    };
    assert_eq!(count(), 0);
    req["confirmation"]["previous"] = json!(0);
    for _ in 0..2 {
        assert!(operator(&db, "intake", req.clone()).status.success());
    }
    req["new_project"]["name"] = json!("Overwrite");
    assert!(!operator(&db, "intake", req).status.success());
    assert_eq!(count(), 1);
}

#[test]
fn scoped_worker_requires_attestation_and_stops_after_acceptance() {
    let (dir, s) = setup();
    let db = dir.path().join("map.sqlite");
    assert!(operator(&db,"bind",json!({"token":TOKEN,"binding":binding("worker",4102444800),"host_scope":{"provider":"codex","workspace":"w"}})).status.success());
    let mut sub = bundle();
    assert!(s.submit_bound(&sub, Some(TOKEN)).is_err());
    let goal = json!({"project_id":"p","node_id":"p/module","generation":2,"previous":0,"goal":"JSON","user_ref":"chat/user-1"});
    assert!(operator(&db, "confirm-goal", goal.clone()).status.success());
    let head = || {
        s.connect(false)
            .unwrap()
            .query_row("SELECT max(id) FROM map_checkpoint_index", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap()
    };
    sub.base_checkpoint_rowid = Some(head());
    sub.checkpoint.version = 2;
    sub.resources = Some(support::logical_manifest());
    s.submit_bound(&sub, Some(TOKEN)).unwrap();
    s.accept(
        "p/module",
        2,
        2,
        &sub.checkpoint.digest,
        "accepted",
        "host",
        "test/receipt",
    )
    .unwrap();
    assert!(!s.submit_bound(&sub, Some(TOKEN)).unwrap().created);
    sub.base_checkpoint_rowid = Some(head());
    sub.checkpoint.version = 3;
    sub.submission_key = "unrequested".into();
    assert!(s.submit_bound(&sub, Some(TOKEN)).is_err());
    let mut reused = goal;
    reused["previous"] = s
        .agent_resume(TOKEN, &query(json!({"project_id":"p"})))
        .unwrap()["takeover"]["confirmation_event"]
        .clone();
    reused["goal"] = json!("resume without new user reply");
    assert!(!operator(&db, "confirm-goal", reused).status.success());
}

#[test]
fn structured_risk_evidence_is_separate_from_the_goal_and_other_nodes() {
    let (dir, s) = setup();
    let db = dir.path().join("map.sqlite");
    let base = json!({"project_id":"p","node_id":"p/module","generation":2,"message_ref":"assistant/1","delivered_text":"Budget and electrical safety remain open.","codes":["BUDGET","SAFETY"]});
    assert!(operator(&db, "record-reminder", base).status.success());
    let query_state = || {
        s.agent_resume(TOKEN, &query(json!({"project_id":"p"})))
            .unwrap()["takeover"]
            .clone()
    };
    assert_eq!(query_state()["risks"]["BUDGET"]["reminded"], true);
    assert_eq!(query_state()["risks"]["BUDGET"]["status"], "unknown");
    let mut fact = json!({"project_id":"p","node_id":"p/module","generation":2,"code":"SAFETY","status":"evidence_verified","source_ref":"","detail":"low voltage rail measured"});
    assert!(!operator(&db, "record-risk", fact.clone()).status.success());
    fact["source_ref"] = json!("measurements/rail-1");
    assert!(operator(&db, "record-risk", fact.clone()).status.success());
    assert_eq!(
        query_state()["risks"]["SAFETY"]["status"],
        "evidence_verified"
    );
    assert_eq!(query_state()["action"], "confirm_goal");
    fact["detail"] = json!("different measurement");
    assert!(!operator(&db, "record-risk", fact).status.success());
}

#[test]
fn conflicting_project_hint_requires_resolution_and_unknown_budget_only_holds_relevant_actions() {
    let (dir, _store) = setup();
    let db = dir.path().join("map.sqlite");
    assert!(operator(&db,"bind",json!({"token":TOKEN,"binding":binding("worker",4102444800),"host_scope":{"provider":"codex","workspace":"w"}})).status.success());
    let key =
        json!({"provider":"codex","workspace":"w","task_id":"stable-task","project_hint":"other"});
    let out = operator(&db, "lookup-host", key);
    assert!(String::from_utf8_lossy(&out.stderr).contains("hint conflicts"));
    let mut req =
        json!({"project_id":"p","node_id":"p/module","action":"research","kind":"hardware"});
    let assessment = |q: Value| {
        let o = operator(&db, "assess-risk", q);
        assert!(o.status.success());
        serde_json::from_slice::<Value>(&o.stdout).unwrap()
    };
    assert_eq!(assessment(req.clone())["pending"], json!([]));
    req["action"] = json!("procure");
    let state = assessment(req.clone());
    assert!(state["pending"]
        .as_array()
        .unwrap()
        .contains(&json!("BUDGET")));
    for status in ["inferred", "user_confirmed"] {
        let fact = json!({"project_id":"p","node_id":"p/module","generation":2,"code":"BUDGET","status":status,"source_ref":format!("test/{status}"),"detail":"unit target agreed"});
        assert!(operator(&db, "record-risk", fact).status.success());
        assert_eq!(
            assessment(req.clone())["pending"]
                .as_array()
                .unwrap()
                .contains(&json!("BUDGET")),
            status == "inferred"
        );
    }
}

#[test]
fn withdrawn_goal_requires_fresh_reply_and_missing_reminders_name_actual_topics() {
    let (dir, s) = setup();
    let db = dir.path().join("map.sqlite");
    let mut q = json!({"project_id":"p","node_id":"p/module","generation":2,"goal":"CSV","user_ref":"user/1","previous":0});
    let op = |verb: &str, q: Value| {
        let r = operator(&db, verb, q);
        assert!(r.status.success());
        serde_json::from_slice::<Value>(&r.stdout).unwrap()
    };
    let id = op("confirm-goal", q.clone())["event_id"].clone();
    let revoke =
        json!({"project_id":"p","node_id":"p/module","previous":id,"user_ref":"user/withdraw"});
    op("revoke-goal", revoke.clone());
    op("revoke-goal", revoke);
    let request = query(json!({"project_id":"p","task_kind":"hardware"}));
    let state = s.agent_resume(TOKEN, &request).unwrap()["takeover"].clone();
    assert_eq!(state["status_code"], "CONFIRM_GOAL");
    assert!(state["confirmation"].is_null());
    assert!(!operator(&db, "confirm-goal", q.clone()).status.success());
    op(
        "record-reminder",
        json!({"project_id":"p","node_id":"p/module","generation":2,"goal_revision":state["confirmation_event"],"message_ref":"assistant/1","delivered_text":"use case, cost, compatibility, safety and compliance","codes":["USE_CASE","BUDGET","COMPATIBILITY","SAFETY","COMPLIANCE"]}),
    );
    assert_eq!(
        s.agent_resume(TOKEN, &request).unwrap()["takeover"]["missing_reminders"],
        json!(["REGION"])
    );
    let mut sub = bundle();
    sub.checkpoint.version = 2;
    sub.base_checkpoint_rowid = Some(
        s.connect(false)
            .unwrap()
            .query_row("SELECT max(id) FROM map_checkpoint_index", [], |r| r.get(0))
            .unwrap(),
    );
    assert!(matches!(
        s.submit_bound(&sub, Some(TOKEN)),
        Err(development_map::store::Error::Conflict(
            "node goal confirmation required"
        ))
    ));
    q["previous"] = state["confirmation_event"].clone();
    q["user_ref"] = json!("user/new");
    q["goal"] = json!("JSON");
    op("confirm-goal", q);
    assert_eq!(
        s.agent_resume(TOKEN, &request).unwrap()["takeover"]["status_code"],
        "CONTINUE"
    );
    assert_eq!(
        s.agent_resume(TOKEN, &request).unwrap()["takeover"]["missing_reminders"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
}

#[test]
fn revoke_racing_a_worker_serializes_before_or_after_the_complete_write() {
    let (dir, s) = setup();
    let db = dir.path().join("map.sqlite");
    let q = json!({"project_id":"p","node_id":"p/module","generation":2,"previous":0,"user_ref":"user/start","goal":"CSV"});
    let id = serde_json::from_slice::<Value>(&operator(&db, "confirm-goal", q).stdout).unwrap()
        ["event_id"]
        .clone();
    let rev =
        json!({"project_id":"p","node_id":"p/module","previous":id,"user_ref":"user/withdraw"});
    let mut sub = bundle();
    sub.checkpoint.version = 2;
    sub.base_checkpoint_rowid = Some(
        s.connect(false)
            .unwrap()
            .query_row("SELECT max(id) FROM map_checkpoint_index", [], |r| r.get(0))
            .unwrap(),
    );
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| s.submit_bound(&sub, Some(TOKEN)));
        assert!(operator(&db, "revoke-goal", rev.clone()).status.success());
        let result = worker.join().unwrap();
        if let Ok(receipt) = result {
            let revoke: i64 = s
                .connect(false)
                .unwrap()
                .query_row(
                    "SELECT id FROM map_events WHERE kind='user_goal_revoked'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(receipt.event_id < revoke);
        }
    });
    let mut wrong = rev.clone();
    wrong["previous"] = json!(0);
    assert!(!operator(&db, "revoke-goal", wrong).status.success());
    let mut altered = rev;
    altered["user_ref"] = json!("another/turn");
    assert!(!operator(&db, "revoke-goal", altered).status.success());
}

#[tokio::test]
async fn global_http_worker_cannot_write_managed_nodes_before_confirmation_or_after_revocation() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let (dir, s) = setup();
    let db = dir.path().join("map.sqlite");
    let legacy = "legacy-http-worker-token-fixture";
    let app = development_map::web::MapWeb::new(s.clone(), "reader".into())
        .with_write_tokens(legacy.into(), "legacy-manager-token-for-fixture".into())
        .unwrap()
        .router();
    let post = |path: &str, value: Value| {
        let req = Request::post(path)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {legacy}"))
            .body(Body::from(value.to_string()))
            .unwrap();
        app.clone().oneshot(req)
    };
    let sub = bundle();
    assert_eq!(
        post("/v1/map/submissions", json!(sub))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    rc2::migrate_fixture(&s);
    // The same legacy API still works on an unrelated RC node.
    let mut control = sub.clone();
    control.checkpoint.node_id = "other/module".into();
    assert_eq!(
        post("/v1/map/submissions", json!(control))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let routed = "separate-scoped-worker-token-fixture";
    let scope = json!({"provider":"codex","workspace":"w"});
    assert!(operator(
        &db,
        "bind",
        json!({"token":routed,"binding":binding("worker",4102444800),"host_scope":scope})
    )
    .status
    .success());
    assert!(s.submit_bound(&sub, Some(TOKEN)).is_err());
    let artifact = json!({"node_id":"p/module","repo_key":"repo","revision":"fixture","path":"out","kind":"file","provenance":"declared","verification":"draft"});
    for (path, body) in [
        ("/v1/map/checkpoints", json!(sub.checkpoint)),
        ("/v1/map/submissions", json!(sub)),
        ("/v1/map/artifacts", artifact.clone()),
    ] {
        assert_eq!(
            post(path, body).await.unwrap().status(),
            StatusCode::CONFLICT,
            "{path}"
        );
    }
    let goal = json!({"project_id":"p","node_id":"p/module","generation":2,"previous":0,"goal":"JSON","user_ref":"test/user"});
    let result = operator(&db, "confirm-goal", goal);
    assert!(result.status.success());
    let event = serde_json::from_slice::<Value>(&result.stdout).unwrap()["event_id"].clone();
    assert!(operator(
        &db,
        "revoke-goal",
        json!({"project_id":"p","node_id":"p/module","previous":event,"user_ref":"test/withdraw"})
    )
    .status
    .success());
    assert_eq!(
        post("/v1/map/checkpoints", json!(sub.checkpoint))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        post("/v1/map/artifacts", artifact).await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        post("/v1/map/submissions", json!(sub))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    s.read(|tx| {
        assert_eq!(
            tx.query_row(
                "SELECT COUNT(*) FROM map_checkpoint_index WHERE node_id='p/module'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            1
        );
        assert_eq!(
            tx.query_row(
                "SELECT COUNT(*) FROM map_node_artifacts WHERE node_id='p/module'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn fresh_unscoped_worker_requires_confirmation_and_reports_write_gate() {
    let (dir, s) = setup();
    let resume = || {
        s.agent_resume(TOKEN, &query(json!({"project_id":"p"})))
            .unwrap()
    };
    assert_eq!(resume()["capabilities"]["submit"], false);
    let mut sub = bundle();
    assert!(matches!(
        s.submit_bound(&sub, Some(TOKEN)),
        Err(development_map::store::Error::Conflict(
            "node goal confirmation required"
        ))
    ));
    let db = dir.path().join("map.sqlite");
    let out = operator(
        &db,
        "confirm-goal",
        json!({"project_id":"p","node_id":"p/module","generation":2,"previous":0,"goal":"JSON","user_ref":"user/1"}),
    );
    assert!(out.status.success());
    assert_eq!(resume()["capabilities"]["submit"], true);
    sub.base_checkpoint_rowid = resume()["checkpoint"]["rowid"].as_i64();
    sub.checkpoint.version = 2;
    assert!(s.submit_bound(&sub, Some(TOKEN)).unwrap().created);
    let id = resume()["takeover"]["confirmation_event"].clone();
    assert!(operator(
        &db,
        "revoke-goal",
        json!({"project_id":"p","node_id":"p/module","previous":id,"user_ref":"user/2"})
    )
    .status
    .success());
    assert_eq!(resume()["capabilities"]["submit"], false);
}

#[tokio::test]
async fn migrated_accepted_http_task_only_replays_the_exact_receipt() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let (_dir, s) = setup();
    let mut prior = bundle();
    prior.resources = Some(support::logical_manifest());
    let receipt = s.submit(&prior).unwrap();
    s.accept(
        "p/module",
        2,
        1,
        &prior.checkpoint.digest,
        "accepted",
        "host",
        "test/accept",
    )
    .unwrap();
    rc2::migrate_fixture(&s);
    assert_eq!(
        s.agent_resume(TOKEN, &query(json!({"project_id":"p"})))
            .unwrap()["capabilities"]["submit"],
        false
    );
    let legacy = "fixture-legacy-worker-credential-only";
    let app = development_map::web::MapWeb::new(s.clone(), "read".into())
        .with_write_tokens(legacy.into(), "fixture-legacy-manager-credential".into())
        .unwrap()
        .router();
    let post = |path: &str, v: Value| {
        app.clone().oneshot(
            Request::post(path)
                .header("authorization", format!("Bearer {legacy}"))
                .header("content-type", "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
        )
    };
    let replay = post("/v1/map/submissions", json!(prior)).await.unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    let replay: Value = serde_json::from_slice(
        &axum::body::to_bytes(replay.into_body(), 2048)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(replay["result"]["created"], false);
    assert_eq!(replay["result"]["event_id"], receipt.event_id);
    let mut next = prior.clone();
    next.checkpoint.summary = "changed".into();
    assert_eq!(
        post("/v1/map/submissions", json!(next))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    next.checkpoint.version = 2;
    next.submission_key = "next".into();
    for (path, body) in [
        ("/v1/map/checkpoints", json!(next.checkpoint)),
        ("/v1/map/submissions", json!(next)),
        (
            "/v1/map/artifacts",
            json!({"node_id":"p/module","repo_key":"r","revision":"v1","path":"out","kind":"file","provenance":"declared","verification":"draft"}),
        ),
    ] {
        assert_eq!(
            post(path, body).await.unwrap().status(),
            StatusCode::CONFLICT,
            "{path}"
        );
    }
    let db = s.connect(false).unwrap();
    for (table, count) in [("map_checkpoint_index", 1), ("map_node_artifacts", 0)] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            count
        );
    }
}

#[test]
fn new_credential_cannot_inherit_rc2_exception_and_v11_is_conservative() {
    let (_dir, s) = setup();
    rc2::migrate_fixture(&s);
    let replacement = "replacement-worker-not-in-snapshot";
    s.bind_identity(replacement, &binding("worker", 4102444800))
        .unwrap();
    assert!(s.submit_bound(&bundle(), Some(replacement)).is_err());
    assert!(s.submit_bound(&bundle(), None).is_err());
    assert!(s.submit_bound(&bundle(), Some(TOKEN)).unwrap().created);

    let (_dir, v11) = setup();
    v11.connect(false).unwrap().execute_batch("DROP TABLE map_rc2_nodes; DROP TABLE map_rc2_workers; UPDATE map_schema_migrations SET version=11").unwrap();
    v11.migrate().unwrap();
    assert!(v11.submit_bound(&bundle(), Some(TOKEN)).is_err());
    assert_eq!(
        v11.agent_resume(TOKEN, &query(json!({"project_id":"p"})))
            .unwrap()["capabilities"]["submit"],
        false
    );
}

#[test]
fn operator_resource_cli_requires_the_target_project_manager() {
    let (dir, s) = setup();
    let manager = "project-manager-resource-test-token";
    let foreign = "foreign-manager-resource-test-token";
    let mut b = binding("manager", 4102444800);
    s.bind_identity(manager, &b).unwrap();
    b.project_id = "other".into();
    s.bind_identity(foreign, &b).unwrap();
    let call = |token: Option<&str>, request: &Submission| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_mapctl"));
        cmd.arg("submit-resources")
            .arg(dir.path().join("map.sqlite"))
            .env_remove("MAP_AGENT_TOKEN");
        if let Some(token) = token {
            cmd.env("MAP_AGENT_TOKEN", token);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(request).unwrap())
            .unwrap();
        child.wait_with_output().unwrap().status
    };
    assert!(!call(Some(manager), &bundle()).success());
    let out = operator(
        &dir.path().join("map.sqlite"),
        "confirm-goal",
        json!({"project_id":"p","node_id":"p/module","generation":2,"previous":0,"goal":"resources","user_ref":"user/1"}),
    );
    assert!(out.status.success());
    let mut request = bundle();
    request.checkpoint.version = 2;
    request.base_checkpoint_rowid = s
        .agent_resume(TOKEN, &query(json!({"project_id":"p"})))
        .unwrap()["checkpoint"]["rowid"]
        .as_i64();
    for token in [None, Some(TOKEN), Some(foreign)] {
        assert!(!call(token, &request).success());
    }
    assert!(call(Some(manager), &request).success());
}

#[test]
fn archived_write_after_acceptance_does_not_reopen_the_task() {
    let (dir, s) = setup();
    let mut sub = bundle();
    sub.resources = Some(support::logical_manifest());
    let receipt = s.submit(&sub).unwrap();
    s.accept(
        "p/module",
        2,
        1,
        &sub.checkpoint.digest,
        "accepted",
        "host",
        "rc2/accepted",
    )
    .unwrap();
    sub.base_checkpoint_rowid = Some(receipt.checkpoint_rowid);
    sub.submission_key = "rc2-later-checkpoint".into();
    sub.checkpoint.version = 2;
    s.submit(&sub).unwrap(); // RC2 allowed an additional write after acceptance.
    rc2::migrate_fixture(&s);
    let state = s
        .agent_resume(TOKEN, &query(json!({"project_id":"p"})))
        .unwrap();
    assert_eq!(state["takeover"]["action"], "complete");
    assert_eq!(state["capabilities"]["submit"], false);
    assert!(operator(&dir.path().join("map.sqlite"),"confirm-goal",json!({"project_id":"p","node_id":"p/module","generation":2,"previous":0,"goal":"new task scope","user_ref":"user/reopen"})).status.success());
    assert_eq!(
        s.agent_resume(TOKEN, &query(json!({"project_id":"p"})))
            .unwrap()["capabilities"]["submit"],
        true
    );
}
