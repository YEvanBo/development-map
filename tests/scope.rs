use development_map::{
    identity::Binding,
    scope::{ScopeItem, ScopeReport, ScopeRequest},
    store::{MapStore, NodeInput},
};
const TOKEN: &str = "fixture-worker-token-at-least-24-chars";
fn setup() -> (tempfile::TempDir, MapStore, i64) {
    let d = tempfile::tempdir().unwrap();
    let s = MapStore::new(d.path().join("map.sqlite"));
    s.migrate().unwrap();
    s.project("p", "P", "fixture").unwrap();
    s.adopt_architecture("p", "v1", "fixture", "1234567890123456")
        .unwrap();
    for n in ["a", "b"] {
        s.node(&NodeInput {
            id: n.into(),
            project_id: "p".into(),
            parent_id: None,
            kind: "module".into(),
            title: n.into(),
            goal: "JSON".into(),
            sort_key: n.into(),
            plan_state: "active".into(),
            owner: Some("worker".into()),
            generation: 1,
            architecture_ref: None,
        })
        .unwrap();
    }
    let rev = s
        .confirm_goal(&development_map::takeover::GoalConfirmation {
            project_id: "p".into(),
            node_id: "a".into(),
            generation: 1,
            previous: 0,
            goal: "JSON".into(),
            user_ref: "fixture-user-confirmation".into(),
        })
        .unwrap();
    s.bind_identity(
        TOKEN,
        &Binding {
            subject: "fixture".into(),
            project_id: "p".into(),
            task_id: "task-a".into(),
            owner: "worker".into(),
            role: "worker".into(),
            node_id: Some("a".into()),
            generation: Some(1),
            source_ref: "fixture".into(),
            expires_at: 4102444800,
        },
    )
    .unwrap();
    (d, s, rev)
}
fn item(resource: &str) -> ScopeItem {
    ScopeItem {
        action: "develop".into(),
        project: "p".into(),
        resource: resource.into(),
        scope: "implementation".into(),
    }
}
fn request() -> ScopeRequest {
    ScopeRequest {
        project_id: "p".into(),
        node_id: "a".into(),
        operations: vec![item("export")],
    }
}
fn report(rev: i64) -> ScopeReport {
    ScopeReport {
        project_id: "p".into(),
        node_id: "a".into(),
        goal_revision: rev,
        scope_revision: 0,
        approved: true,
        scope: vec![item("export")],
        user_reply: None,
        message_ref: None,
        note: "Agent reports user's initial scope".into(),
    }
}
#[test]
fn ordinary_work_is_not_repeatedly_confirmed() {
    let (_d, s, rev) = setup();
    s.scope_report(TOKEN, &report(rev)).unwrap();
    for action in [
        "develop",
        "implement",
        "refactor",
        "migrate",
        "fix",
        "test",
        "build",
        "format",
        "lint",
    ] {
        let mut q = request();
        q.operations[0].action = action.into();
        assert_eq!(s.scope_check(TOKEN, &q).unwrap()["status"], "IN_SCOPE");
    }
}
#[test]
fn scope_capacity_is_reported_before_asking_user() {
    let (_d, s, rev) = setup();
    let mut r = report(rev);
    r.scope = (0..64).map(|i| item(&format!("module-{i}"))).collect();
    let saved = s.scope_report(TOKEN, &r).unwrap();
    let overflow = s.scope_check(TOKEN, &request()).unwrap();
    assert_eq!(overflow["status"], "BLOCKED");
    assert_eq!(overflow["reason"], "SCOPE_CAPACITY_EXCEEDED");
    assert_eq!(overflow["capacity"]["limit"], 64);
    assert_eq!(overflow["capacity"]["required"], 65);
    assert!(overflow["required_user_action"].is_null());
    assert!(overflow["suggested_scope"].is_null());
    assert_eq!(overflow["current"], saved["current"]);
    let q = ScopeRequest {
        operations: vec![item("module-0")],
        ..request()
    };
    assert_eq!(s.scope_check(TOKEN, &q).unwrap()["status"], "IN_SCOPE");
}
#[test]
fn cli_help_lists_scope_commands() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mapctl"))
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&out.stderr);
    assert!(help.contains("scope-check"), "{help}");
    assert!(help.contains("scope-report"), "{help}");
}
#[test]
fn resume_distinguishes_effective_consent_from_latest_refusal_quote() {
    let (_d, s, rev) = setup();
    let mut r = report(rev);
    r.user_reply = Some("Only export is approved".into());
    let initial = s.scope_report(TOKEN, &r).unwrap();
    r.scope_revision = initial["current"]["scope_revision"].as_i64().unwrap();
    r.approved = false;
    r.scope = vec![item("shared")];
    r.user_reply = Some("No shared-module edit; deliver only what is finished".into());
    r.note = "User refused the proposed extension".into();
    s.scope_report(TOKEN, &r).unwrap();
    let resumed = s
        .agent_resume(
            TOKEN,
            &serde_json::from_value(serde_json::json!({"project_id":"p"})).unwrap(),
        )
        .unwrap();
    assert_eq!(
        resumed["task_scope"]["user_reply"],
        "Only export is approved"
    );
    assert_eq!(
        resumed["task_scope"]["last_decision_user_reply"],
        r.user_reply.unwrap()
    );
    assert_eq!(resumed["task_scope"]["scope"], initial["current"]["scope"]);
}
#[test]
fn expansion_reports_exact_difference_and_true_persists() {
    let (_d, s, rev) = setup();
    let first = s.scope_report(TOKEN, &report(rev)).unwrap();
    let mut q = request();
    q.operations.push(item("shared-format"));
    let v = s.scope_check(TOKEN, &q).unwrap();
    assert_eq!(v["status"], "NEEDS_USER_CONFIRMATION");
    assert_eq!(v["scope_difference"][0]["resource"], "shared-format");
    assert_eq!(v["suggested_scope"].as_array().unwrap().len(), 2);
    let mut r = report(rev);
    r.scope_revision = first["current"]["scope_revision"].as_i64().unwrap();
    r.scope = q.operations.clone();
    s.scope_report(TOKEN, &r).unwrap();
    assert_eq!(s.scope_check(TOKEN, &q).unwrap()["status"], "IN_SCOPE");
    let restored = s
        .agent_resume(
            TOKEN,
            &serde_json::from_value(serde_json::json!({"project_id":"p"})).unwrap(),
        )
        .unwrap();
    assert_eq!(restored["task_scope"]["scope"].as_array().unwrap().len(), 2);
}
#[test]
fn false_or_pending_keeps_original_work_available() {
    let (_d, s, rev) = setup();
    let mut first = s.scope_report(TOKEN, &report(rev)).unwrap();
    let mut q = request();
    q.operations = vec![item("other")];
    for _choice in ["refused", "cancelled", "unclear"] {
        let mut r = report(rev);
        r.approved = false;
        r.scope = q.operations.clone();
        r.scope_revision = first["current"]["scope_revision"].as_i64().unwrap();
        first = s.scope_report(TOKEN, &r).unwrap();
        assert_eq!(
            s.scope_check(TOKEN, &q).unwrap()["status"],
            "NEEDS_USER_CONFIRMATION"
        );
        assert_eq!(
            s.scope_check(TOKEN, &request()).unwrap()["status"],
            "IN_SCOPE"
        );
    } // No report is the pending/no-reply case.
    assert_eq!(
        s.scope_check(TOKEN, &q).unwrap()["status"],
        "NEEDS_USER_CONFIRMATION"
    );
}
#[test]
fn restricted_scope_is_not_silently_expanded() {
    let (_d, s, rev) = setup();
    let mut r = report(rev);
    r.scope.push(item("helper"));
    let first = s.scope_report(TOKEN, &r).unwrap();
    r.scope_revision = first["current"]["scope_revision"].as_i64().unwrap();
    r.scope = vec![item("export")];
    s.scope_report(TOKEN, &r).unwrap();
    let mut q = request();
    q.operations = vec![item("helper")];
    assert_eq!(
        s.scope_check(TOKEN, &q).unwrap()["status"],
        "NEEDS_USER_CONFIRMATION"
    );
}
#[test]
fn cross_project_is_reminder_not_credential() {
    let (_d, s, rev) = setup();
    s.scope_report(TOKEN, &report(rev)).unwrap();
    let mut q = request();
    q.operations[0].project = "q".into();
    assert_eq!(
        s.scope_check(TOKEN, &q).unwrap()["status"],
        "NEEDS_USER_CONFIRMATION"
    );
    q.project_id = "q".into();
    assert!(s.scope_check(TOKEN, &q).is_err());
    q = request();
    q.node_id = "b".into();
    assert!(s.scope_check(TOKEN, &q).is_err());
}
#[test]
fn stale_goal_or_scope_never_overwrites_newer_decision() {
    let (_d, s, rev) = setup();
    s.scope_report(TOKEN, &report(rev)).unwrap();
    assert!(s.scope_report(TOKEN, &report(rev)).is_err());
    s.confirm_goal(&development_map::takeover::GoalConfirmation {
        project_id: "p".into(),
        node_id: "a".into(),
        generation: 1,
        previous: rev,
        goal: "new goal".into(),
        user_ref: "new-user-reply".into(),
    })
    .unwrap();
    assert!(s.scope_report(TOKEN, &report(rev)).is_err());
    assert_eq!(
        s.scope_check(TOKEN, &request()).unwrap()["status"],
        "NEEDS_USER_CONFIRMATION"
    );
}
#[test]
fn unknown_empty_scope_and_non_boolean_cannot_be_assumed_approved() {
    let (_d, s, _rev) = setup();
    let mut q = request();
    q.operations.clear();
    assert_eq!(
        s.scope_check(TOKEN, &q).unwrap()["status"],
        "NEEDS_USER_CONFIRMATION"
    );
    assert!(serde_json::from_value::<ScopeReport>(serde_json::json!({"project_id":"p","node_id":"a","goal_revision":1,"scope_revision":0,"approved":"true","scope":[],"note":""})).is_err());
}
#[test]
fn revocation_and_accepted_cannot_be_bypassed() {
    for accepted in [false, true] {
        let (_d, s, rev) = setup();
        if accepted {
            s.connect(false).unwrap().execute_batch("INSERT INTO map_acceptances(node_id,generation,version,digest,decision,evidence_level,source_ref,created_at) SELECT node_id,generation,version,digest,'accepted','fixture','fixture',unixepoch() FROM map_checkpoint_index WHERE node_id='a';").unwrap();
        } else {
            s.revoke_goal(&serde_json::from_value(serde_json::json!({"project_id":"p","node_id":"a","previous":rev,"user_ref":"revoked"})).unwrap()).unwrap();
        }
        assert_eq!(
            s.scope_check(TOKEN, &request()).unwrap()["status"],
            "BLOCKED"
        );
        assert!(s.scope_report(TOKEN, &report(rev)).is_err());
    }
}
#[test]
fn labels_are_matched_as_tuples_not_cartesian_permissions() {
    let (_d, s, rev) = setup();
    let mut r = report(rev);
    let mut second = item("database");
    second.action = "delete".into();
    second.scope = "important_data".into();
    r.scope.push(second);
    s.scope_report(TOKEN, &r).unwrap();
    let mut q = request();
    q.operations[0].action = "delete".into();
    q.operations[0].scope = "important_data".into();
    assert_eq!(
        s.scope_check(TOKEN, &q).unwrap()["status"],
        "NEEDS_USER_CONFIRMATION"
    );
}
#[test]
fn refusal_fences_old_approval() {
    let (_d, s, rev) = setup();
    let mut r = report(rev);
    r.approved = false;
    s.scope_report(TOKEN, &r).unwrap();
    assert!(s.scope_report(TOKEN, &report(rev)).is_err());
}
#[test]
fn recovery_retains_exact_user_limit() {
    let (_d, s, rev) = setup();
    let mut r = report(rev);
    r.scope = vec![item("shared-format:null-handling")];
    r.note = "只改空值，不改公共接口".into();
    r.user_reply = Some(r.note.clone());
    s.scope_report(TOKEN, &r).unwrap();
    let v = s
        .agent_resume(
            TOKEN,
            &serde_json::from_value(serde_json::json!({"project_id":"p"})).unwrap(),
        )
        .unwrap();
    assert_eq!(v["task_scope"]["note"], r.note);
    assert_eq!(v["task_scope"]["user_reply"], r.user_reply.unwrap());
}
#[test]
fn all_declared_material_changes_differ() {
    let (_d, s, rev) = setup();
    s.scope_report(TOKEN, &report(rev)).unwrap();
    for boundary in [
        "public_interface",
        "architecture",
        "cost",
        "external_dependency",
        "important_data",
        "deployment",
        "publication",
        "external_effect",
        "goal_change",
        "reopen",
    ] {
        let mut q = request();
        q.operations[0].scope = boundary.into();
        assert_eq!(
            s.scope_check(TOKEN, &q).unwrap()["status"],
            "NEEDS_USER_CONFIRMATION"
        );
    }
}
#[test]
fn another_valid_node_does_not_inherit_scope() {
    let (_d, s, rev) = setup();
    s.scope_report(TOKEN, &report(rev)).unwrap();
    s.confirm_goal(&development_map::takeover::GoalConfirmation {
        project_id: "p".into(),
        node_id: "b".into(),
        generation: 1,
        previous: 0,
        goal: "JSON".into(),
        user_ref: "user-b".into(),
    })
    .unwrap();
    let other = "fixture-other-token-at-least-24-chars";
    s.bind_identity(
        other,
        &Binding {
            subject: "other".into(),
            project_id: "p".into(),
            task_id: "task-b".into(),
            owner: "worker".into(),
            role: "worker".into(),
            node_id: Some("b".into()),
            generation: Some(1),
            source_ref: "fixture".into(),
            expires_at: 4102444800,
        },
    )
    .unwrap();
    let mut q = request();
    q.node_id = "b".into();
    assert_eq!(
        s.scope_check(other, &q).unwrap()["status"],
        "NEEDS_USER_CONFIRMATION"
    );
}
#[test]
fn concurrent_scope_decisions_do_not_overwrite() {
    let (_d, s, rev) = setup();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let s = s.clone();
            let b = barrier.clone();
            std::thread::spawn(move || {
                b.wait();
                s.scope_report(TOKEN, &report(rev)).is_ok()
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|success| *success)
            .count(),
        1
    );
}
#[test]
fn resume_combines_facts_with_measured_call_bytes() {
    let (_d, s, rev) = setup();
    s.scope_report(TOKEN, &report(rev)).unwrap();
    let q: development_map::agent::Resume =
        serde_json::from_value(serde_json::json!({"project_id":"p"})).unwrap();
    let combined = s.agent_resume(TOKEN, &q).unwrap();
    for key in [
        "takeover",
        "task_scope",
        "checkpoint",
        "identity",
        "node",
        "capabilities",
        "gaps",
    ] {
        assert!(combined.get(key).is_some(), "{key}");
    }
    let extra = s.scope_check(TOKEN, &request()).unwrap();
    let repeated = s.agent_resume(TOKEN, &q).unwrap();
    let n = |v: &serde_json::Value| serde_json::to_vec(v).unwrap().len();
    let input = serde_json::json!({"project_id":"p"});
    let check = serde_json::json!({"project_id":"p","node_id":"a","operations":[{"action":"develop","project":"p","resource":"export","scope":"implementation"}]});
    println!(
        "METRICS {}",
        serde_json::json!({"mode":"illustrative redundant client versus single composed resume; not historical release benchmark","redundant":{"core_calls":3,"input_bytes":2*n(&input)+n(&check),"output_bytes":n(&combined)+n(&extra)+n(&repeated)},"composed":{"core_calls":1,"input_bytes":n(&input),"output_bytes":n(&combined)},"llm_round_trips":"NOT TESTED","tokens":"NOT MEASURED","cost":"NOT MEASURED"})
    );
}
#[test]
fn mcp_scope_roundtrip_uses_existing_worker_identity() {
    let (_d, s, rev) = setup();
    let mut session = development_map::mcp::Session::default();
    session.handle(&s,TOKEN,serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"scope-fixture","version":"1"}}}));
    session.handle(
        &s,
        TOKEN,
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    let r = report(rev);
    let args = serde_json::json!({"project_id":"p","node_id":"a","goal_revision":rev,"scope_revision":0,"approved":true,"scope":[{"action":"develop","project":"p","resource":"export","scope":"implementation"}],"note":r.note,"user_reply":null,"message_ref":null});
    let out=session.handle(&s,TOKEN,serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"map_scope_report","arguments":args}})).unwrap();
    assert_eq!(out["result"]["isError"], false);
    let out=session.handle(&s,TOKEN,serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"map_scope_check","arguments":{"project_id":"p","node_id":"a","operations":[{"action":"fix","project":"p","resource":"export","scope":"implementation"}]}}})).unwrap();
    let value: serde_json::Value =
        serde_json::from_str(out["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(value["status"], "IN_SCOPE");
}
#[test]
fn v12_and_v13_upgrade_preserve_history_without_scope_grants() {
    for old in [12, 13] {
        let (_d, s, _) = setup();
        let db = s.connect(false).unwrap();
        if old == 13 {
            db.execute_batch(include_str!("fixtures/retired-approval-v13.sql"))
                .unwrap();
            db.execute("INSERT INTO map_approval_requests VALUES('old','old-nonce','p','a','{}',4102444800,'pending',NULL,NULL)",[]).unwrap();
        }
        let before: i64 = db
            .query_row("SELECT count(*) FROM map_checkpoint_index", [], |r| {
                r.get(0)
            })
            .unwrap();
        db.execute(
            "UPDATE map_schema_migrations SET version=? WHERE version=14",
            [old],
        )
        .unwrap();
        drop(db);
        s.migrate().unwrap();
        let db = s.connect(true).unwrap();
        assert_eq!(
            before,
            db.query_row("SELECT count(*) FROM map_checkpoint_index", [], |r| r
                .get::<_, i64>(0))
                .unwrap()
        );
        development_map::recovery::integrity(&db).unwrap();
        if old == 13 {
            assert_eq!(
                db.query_row("SELECT count(*) FROM map_approval_requests", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            s.scope_check(TOKEN, &request()).unwrap()["status"],
            "NEEDS_USER_CONFIRMATION"
        );
    }
}
#[test]
fn cli_scope_check_and_report() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let (d, _s, rev) = setup();
    let run = |verb: &str, input: serde_json::Value| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mapctl"))
            .arg(verb)
            .arg(d.path().join("map.sqlite"))
            .env("MAP_AGENT_TOKEN", TOKEN)
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
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let scope = serde_json::json!([{"action":"develop","project":"p","resource":"export","scope":"implementation"}]);
    assert_eq!(
        run(
            "scope-check",
            serde_json::json!({"project_id":"p","node_id":"a","operations":scope})
        )["status"],
        "NEEDS_USER_CONFIRMATION"
    );
    run(
        "scope-report",
        serde_json::json!({"project_id":"p","node_id":"a","goal_revision":rev,"scope_revision":0,"approved":true,"scope":scope,"note":"fixture user said yes"}),
    );
    assert_eq!(
        run(
            "scope-check",
            serde_json::json!({"project_id":"p","node_id":"a","operations":scope})
        )["status"],
        "IN_SCOPE"
    );
}
