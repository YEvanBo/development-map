//! Real process death while SQLite owns an uncommitted transaction.
use development_map::{
    recovery,
    store::{MapStore, NodeInput},
};
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[test]
fn crash_child() {
    let Ok(path) = std::env::var("DEVMAP_CRASH_DB") else {
        return;
    };
    let s = MapStore::new(path);
    let mut db = s.connect(false).unwrap();
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    tx.execute(
        "UPDATE map_dev_nodes SET title='UNCOMMITTED' WHERE id='node'",
        [],
    )
    .unwrap();
    std::fs::write(
        std::env::var("DEVMAP_CRASH_MARKER").unwrap(),
        "transaction active",
    )
    .unwrap();
    use std::io::Read;
    let mut byte = [0];
    std::io::stdin().read_exact(&mut byte).unwrap();
    tx.commit().unwrap(); // Parent kills before this is reachable.
}
#[test]
fn killed_writer_rolls_back_and_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("map.sqlite");
    let marker = dir.path().join("active");
    let s = MapStore::new(&path);
    s.migrate().unwrap();
    s.project("p", "P", "test").unwrap();
    s.adopt_architecture("p", "v1", "design", "1234567890123456")
        .unwrap();
    s.node(&NodeInput {
        id: "node".into(),
        project_id: "p".into(),
        parent_id: None,
        kind: "module".into(),
        title: "COMMITTED".into(),
        goal: "test".into(),
        sort_key: "1".into(),
        plan_state: "active".into(),
        owner: None,
        generation: 0,
        architecture_ref: None,
    })
    .unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_child", "--nocapture"])
        .env("DEVMAP_CRASH_DB", &path)
        .env("DEVMAP_CRASH_MARKER", &marker)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() {
        if Instant::now() > deadline {
            child.kill().ok();
            panic!("child never acquired transaction")
        };
        if let Some(status) = child.try_wait().unwrap() {
            panic!("writer exited before interruption: {status}")
        };
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    let reopened = MapStore::new(path);
    let db = reopened.connect(true).unwrap();
    recovery::integrity(&db).unwrap();
    let title: String = db
        .query_row("SELECT title FROM map_dev_nodes WHERE id='node'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(title, "COMMITTED");
    // Writers work normally after the killed transaction releases the lock.
    reopened
        .project("p", "Recovered", "test-after-crash")
        .unwrap();
}
