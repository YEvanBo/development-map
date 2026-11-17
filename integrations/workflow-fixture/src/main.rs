//! Test-only driver. Always creates its own temporary database, no path input.
use development_map::{
    identity::Binding,
    mcp::Session,
    store::{MapStore, NodeInput},
};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
fn main() {
    let temp = tempfile::tempdir().unwrap();
    let s = MapStore::new(temp.path().join("fixture.sqlite"));
    s.migrate().unwrap();
    s.project("p", "fixture", "fixture").unwrap();
    s.adopt_architecture("p", "v1", "fixture", "1234567890123456")
        .unwrap();
    s.node(&NodeInput {
        id: "n".into(),
        project_id: "p".into(),
        parent_id: None,
        kind: "module".into(),
        title: "fixture".into(),
        goal: "old CSV".into(),
        sort_key: "1".into(),
        plan_state: "active".into(),
        owner: Some("fixture".into()),
        generation: 1,
        architecture_ref: None,
    })
    .unwrap();
    let token = "synthetic-workflow-worker-token-123456789";
    s.bind_identity(
        token,
        &Binding {
            subject: "fixture".into(),
            project_id: "p".into(),
            task_id: "t".into(),
            owner: "fixture".into(),
            role: "worker".into(),
            node_id: Some("n".into()),
            generation: Some(1),
            source_ref: "fixture".into(),
            expires_at: 4102444800,
        },
    )
    .unwrap();
    let mut session = Session::default();
    session.handle(&s,token,json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}}));
    session.handle(
        &s,
        token,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    for line in io::stdin().lock().lines() {
        let q: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let out = match q["fixture"].as_str() {
            Some("confirm") => {
                json!({"event":s.confirm_goal(&serde_json::from_value(q["value"].clone()).unwrap()).unwrap()})
            }
            Some("revoke") => {
                s.revoke_goal(&serde_json::from_value(q["value"].clone()).unwrap())
                    .unwrap();
                json!({"ok":true})
            }
            Some("accept") => {
                s.accept(
                    "n",
                    1,
                    q["version"].as_i64().unwrap(),
                    q["digest"].as_str().unwrap(),
                    "accepted",
                    "host",
                    "fixture",
                )
                .unwrap();
                json!({"ok":true})
            }
            None => session
                .handle(
                    &s,
                    token,
                    json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":q}),
                )
                .unwrap()["result"]
                .clone(),
            _ => panic!("unsupported fixture operation"),
        };
        println!("{out}");
        io::stdout().flush().unwrap();
    }
}
