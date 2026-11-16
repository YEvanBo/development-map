#[path = "support/rc2.rs"]
mod rc2;
mod support;
use development_map::{
    identity::{authorize, Binding},
    import::{LegacyManifest, LegacyNode},
    navigation, recovery,
    store::{ArtifactInput, CheckpointInput, MapStore, NodeInput},
    submission::Submission,
};
use tempfile::tempdir;
fn setup() -> (tempfile::TempDir, MapStore) {
    let d = tempdir().unwrap();
    let s = MapStore::new(d.path().join("map.sqlite"));
    s.migrate().unwrap();
    s.project("p", "P", "source").unwrap();
    s.adopt_architecture("p", "v1", "design", "1234567890123456")
        .unwrap();
    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .arg(d.path())
        .status()
        .unwrap()
        .success());
    s.register_repository("r", d.path(), "test").unwrap();
    s.register_project_repository("p", "r").unwrap();
    (d, s)
}
fn node(id: &str, parent: Option<&str>) -> NodeInput {
    NodeInput {
        id: id.into(),
        project_id: "p".into(),
        parent_id: parent.map(str::to_string),
        kind: "module".into(),
        title: id.into(),
        goal: "".into(),
        sort_key: id.into(),
        plan_state: "active".into(),
        owner: Some("worker".into()),
        generation: 1,
        architecture_ref: None,
    }
}
fn submission() -> Submission {
    Submission {
        base_checkpoint_rowid: None,
        submission_key: "s1".into(),
        checkpoint: CheckpointInput {
            node_id: "m".into(),
            owner: "worker".into(),
            generation: 1,
            version: 1,
            digest: "1234567890123456".into(),
            summary: "tested".into(),
            source_ref: "cp.json".into(),
            git_revision: None,
        },
        resources: None,
        artifacts: vec![ArtifactInput {
            node_id: "m".into(),
            repo_key: "r".into(),
            revision: "abc".into(),
            path: "src/a.rs".into(),
            kind: "file".into(),
            provenance: "declared".into(),
            verification: "unverified".into(),
            content_digest: None,
        }],
    }
}
#[test]
fn submission_is_atomic_idempotent_and_detects_payload_change() {
    let (_d, s) = setup();
    s.node(&node("m", None)).unwrap();
    let before: i64 = s
        .connect(true)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM map_events", [], |r| r.get(0))
        .unwrap();
    let mut bad = submission();
    bad.artifacts.push(ArtifactInput {
        path: "../escape".into(),
        ..bad.artifacts[0].clone()
    });
    assert!(s.submit(&bad).is_err());
    let db = s.connect(true).unwrap();
    for table in [
        "map_checkpoint_index",
        "map_artifacts",
        "map_submissions",
        "map_checkpoint_artifacts",
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        )
    }
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM map_events", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        before
    );
    let first = s.submit(&submission()).unwrap();
    let retry = s.submit(&submission()).unwrap();
    assert!(first.created);
    assert!(!retry.created);
    assert_eq!(first.event_id, retry.event_id);
    let mut changed = submission();
    changed.checkpoint.summary = "changed".into();
    assert!(s.submit(&changed).is_err());
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM map_checkpoint_artifacts", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        1
    );
}
#[test]
fn navigation_move_rollup_pages_history_and_acceptance() {
    let (_d, s) = setup();
    s.node(&node("a", None)).unwrap();
    s.node(&node("b", None)).unwrap();
    s.node(&node("m", Some("a"))).unwrap();
    let root = "architecture:p:v1";
    let page = s
        .read(|tx| navigation::children(tx, "p", None, root, "", 1, None))
        .unwrap();
    assert_eq!(page["has_more"], true);
    let next = page["next"].as_str().unwrap();
    assert_eq!(
        s.read(|tx| navigation::children(tx, "p", None, root, next, 1, None))
            .unwrap()["items"][0]["node_ref"],
        "dev:b"
    );
    let mut submitted = submission();
    submitted.resources = Some(support::logical_manifest());
    s.submit(&submitted).unwrap();
    assert!(s
        .read(|tx| navigation::children(tx, "p", None, root, next, 1, None))
        .is_err());
    s.accept("m", 1, 1, "1234567890123456", "accepted", "host", "review")
        .unwrap();
    let sum = s
        .read(|tx| navigation::summary(tx, "p", None, "dev:a"))
        .unwrap();
    assert_eq!(sum["by_kind"][0]["accepted"], 1);
    s.node(&node("m", Some("b"))).unwrap();
    assert_eq!(
        s.read(|tx| navigation::summary(tx, "p", None, "dev:a"))
            .unwrap()["by_kind"][0]["accepted"],
        0
    );
    let f = s
        .read(|tx| navigation::focus(tx, "p", None, "dev:m"))
        .unwrap();
    assert_eq!(f["breadcrumbs"][2]["node_ref"], "dev:b");
    assert_eq!(f["breadcrumbs"].as_array().unwrap().len(), 4);
    assert!(s
        .read(|tx| navigation::ancestors(tx, "p", "v1", "dev:m", Some("dev:a"), 1))
        .is_err());
    s.adopt_architecture("p", "v2", "design2", "abcdefghijklmnop")
        .unwrap();
    assert_eq!(
        s.read(|tx| navigation::focus(tx, "p", Some("v1"), "dev:m"))
            .unwrap()["revision"],
        "v1"
    );
    assert_eq!(
        s.read(|tx| navigation::focus(tx, "p", None, "dev:m"))
            .unwrap()["node"]["acceptance_state"],
        "stale"
    );
}
#[test]
fn binding_revocation_expiry_and_handoff() {
    let (_d, s) = setup();
    s.node(&node("m", None)).unwrap();
    let token = "worker-token-longer-than-24-characters";
    let b = Binding {
        subject: "account:worker".into(),
        project_id: "p".into(),
        task_id: "task-real-id".into(),
        owner: "worker".into(),
        role: "worker".into(),
        node_id: Some("m".into()),
        generation: Some(1),
        source_ref: "host-assignment".into(),
        expires_at: 4102444800,
    };
    s.bind_identity(token, &b).unwrap();
    rc2::migrate_fixture(&s);
    assert!(s
        .read(|tx| authorize(tx, token, "other", Some("m"), "worker"))
        .is_err());
    assert!(s
        .read(|tx| authorize(tx, token, "p", Some("m"), "manager"))
        .is_err());
    s.submit_bound(&submission(), Some(token)).unwrap();
    let mut handoff = node("m", None);
    handoff.generation = 2;
    handoff.owner = Some("next".into());
    s.node(&handoff).unwrap();
    assert!(s.submit_bound(&submission(), Some(token)).is_err());
    s.revoke_identity(token).unwrap();
    assert!(s
        .read(|tx| authorize(tx, token, "p", Some("m"), "worker"))
        .is_err());
    let mut expired = b;
    expired.role = "reader".into();
    expired.node_id = None;
    expired.generation = None;
    expired.expires_at = 1;
    s.bind_identity("expired-token-longer-than-24-characters", &expired)
        .unwrap();
    assert!(s
        .read(|tx| authorize(
            tx,
            "expired-token-longer-than-24-characters",
            "p",
            None,
            "reader"
        ))
        .is_err());
}
#[test]
fn migration_backup_restore_and_rebuild_preserve_host_tables() {
    let d = tempdir().unwrap();
    let path = d.path().join("old.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../src/schema.sql")).unwrap();
    db.execute_batch("CREATE TABLE host_data(value TEXT);INSERT INTO host_data VALUES('keep');PRAGMA user_version=42;INSERT INTO map_projects VALUES('p','P','legacy',0);").unwrap();
    drop(db);
    let s = MapStore::new(&path);
    s.migrate().unwrap();
    s.migrate().unwrap();
    let backup = d.path().join("backup.sqlite");
    s.backup_to(&backup).unwrap();
    assert!(s.backup_to(&backup).is_err());
    let restored = recovery::restore(&backup, &d.path().join("restored.sqlite")).unwrap();
    let db = restored.connect(false).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        42
    );
    assert_eq!(
        db.query_row("SELECT value FROM host_data", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    db.execute("DELETE FROM map_navigation", []).unwrap();
    restored.rebuild_projections().unwrap();
    assert_eq!(
        restored
            .read(|tx| navigation::focus(tx, "p", None, "project:p"))
            .unwrap()["node"]["title"],
        "P"
    );
    recovery::integrity(&db).unwrap();
}
#[test]
fn import_preserves_unadopted_and_rejects_cycles_atomically() {
    let d = tempdir().unwrap();
    let s = MapStore::new(d.path().join("map.sqlite"));
    s.migrate().unwrap();
    let mut m = LegacyManifest {
        project_id: "legacy".into(),
        name: "Legacy".into(),
        source_ref: "ledger".into(),
        nodes: vec![LegacyNode {
            id: "a".into(),
            parent_id: None,
            title: "a".into(),
            source_status: "verified".into(),
            source_owner: Some("old".into()),
        }],
    };
    assert!(s.import_manifest(&m).unwrap());
    assert!(!s.import_manifest(&m).unwrap());
    let focus = s
        .read(|tx| navigation::focus(tx, "legacy", None, "dev:a"))
        .unwrap();
    assert_eq!(focus["node"]["plan_state"], "legacy");
    assert_eq!(focus["node"]["acceptance_state"], "pending");
    assert_eq!(focus["core_architecture_ref"], serde_json::Value::Null);
    m.nodes[0].parent_id = Some("a".into());
    assert!(s.import_manifest(&m).is_err());
    assert_eq!(
        s.read(|tx| navigation::focus(tx, "legacy", None, "dev:a"))
            .unwrap()["parent_ref"],
        "project:legacy"
    );
}
#[test]
fn git_rename_unmerged_branch_and_repeated_scan() {
    use std::process::Command;
    let (d, s) = setup();
    let root = d.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.email", "test@example.invalid"]);
    git(&["config", "user.name", "Test"]);
    std::fs::create_dir(root.join("old")).unwrap();
    std::fs::write(root.join("old/file.txt"), "content").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "initial"]);
    let from = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "-b", "unmerged"]);
    git(&["mv", "old", "new"]);
    git(&["commit", "-m", "rename"]);
    let to = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "main"]);
    s.node(&node("m", None)).unwrap();
    s.register_repository("r", &root, "fixture").unwrap();
    s.artifact(&ArtifactInput {
        node_id: "m".into(),
        repo_key: "r".into(),
        revision: from.clone(),
        path: "old".into(),
        kind: "directory".into(),
        provenance: "declared".into(),
        verification: "unverified".into(),
        content_digest: None,
    })
    .unwrap();
    s.register_repository("r", &root, "fixture").unwrap();
    assert_eq!(s.scan_git("r", &from, &to).unwrap(), 1);
    assert_eq!(s.scan_git("r", &from, &to).unwrap(), 1);
    let changes = s.git_changes("r", &from, &to, -1, 1).unwrap();
    assert_eq!(changes["items"][0]["old_path"], "old/file.txt");
    assert_eq!(changes["items"][0]["path"], "new/file.txt");
    let links = s
        .read(|tx| development_map::store::lookup(tx, "r", &to, "new/file.txt", 10))
        .unwrap();
    assert_eq!(links["items"][0]["node_id"], "m");
    assert_eq!(links["items"][0]["provenance"], "observed");
    assert_eq!(
        s.read(|tx| development_map::store::lookup(tx, "r", &from, "old/file.txt", 10))
            .unwrap()["items"][0]["provenance"],
        "declared"
    );
    assert_eq!(git(&["rev-parse", "HEAD"]), from);
}
#[test]
fn early_schema_upgrade_preserves_architecture_and_links() {
    let d = tempdir().unwrap();
    let p = d.path().join("early.sqlite");
    let db = rusqlite::Connection::open(&p).unwrap();
    db.execute_batch(include_str!("fixtures/pre-versioned-schema.sql"))
        .unwrap();
    db.execute_batch("INSERT INTO map_projects VALUES('p','P','old',0);INSERT INTO map_architectures VALUES('p','v1','design','1234567890123456',0);INSERT INTO map_arch_nodes(id,project_id,arch_version,title,kind,source_ref,sort_key) VALUES('a','p','v1','A','component','design','a');INSERT INTO map_dev_nodes(id,project_id,kind,title,goal,sort_key,plan_state,updated_at) VALUES('m','p','module','M','','m','active',0);INSERT INTO map_arch_traces VALUES('a','m','primary','link');").unwrap();
    drop(db);
    let legacy = rusqlite::Connection::open(&p).unwrap();
    legacy
        .execute(
            "UPDATE map_dev_nodes SET owner='worker',generation=1 WHERE id='m'",
            [],
        )
        .unwrap();
    legacy.execute("INSERT INTO map_checkpoint_index(node_id,owner,generation,version,digest,summary,source_ref,arch_version,created_at) VALUES('m','worker',1,1,'1234567890123456','tested','cp.json','v1',0)",[]).unwrap();
    drop(legacy);
    let s = MapStore::new(&p);
    s.migrate().unwrap();
    assert!(!s.checkpoint(&submission().checkpoint).unwrap());
    let f = s
        .read(|tx| navigation::focus(tx, "p", None, "dev:m"))
        .unwrap();
    assert_eq!(f["parent_ref"], "arch:a");
    assert_eq!(f["revision"], "v1");
    recovery::integrity(&s.connect(true).unwrap()).unwrap();
}
#[test]
fn traces_and_interfaces_page_without_duplicate_or_missing_relations() {
    let (_d, s) = setup();
    s.architecture_node("a", "p", None, "A", "component", "design", "a")
        .unwrap();
    s.architecture_node("b", "p", None, "B", "component", "design", "b")
        .unwrap();
    s.node(&node("m", None)).unwrap();
    s.trace("a", "m", "primary", "s").unwrap();
    s.trace("a", "m", "implements", "s").unwrap();
    let p = s
        .read(|tx| development_map::relations::traces(tx, "a", "v1", "", 1))
        .unwrap();
    assert_eq!(p["has_more"], true);
    let q = s
        .read(|tx| {
            development_map::relations::traces(tx, "a", "v1", p["next"].as_str().unwrap(), 1)
        })
        .unwrap();
    assert_eq!(q["has_more"], false);
    assert_ne!(p["items"][0]["relation"], q["items"][0]["relation"]);
    assert!(s
        .read(|tx| development_map::relations::traces(
            tx,
            "b",
            "v1",
            p["next"].as_str().unwrap(),
            1
        ))
        .is_err());
}

#[test]
fn handoff_compare_and_swap_rejects_stale_manager() {
    let (_d, s) = setup();
    s.node(&node("m", None)).unwrap();
    let mut submitted = submission();
    submitted.resources = Some(support::logical_manifest());
    s.submit(&submitted).unwrap();
    s.accept("m", 1, 1, "1234567890123456", "accepted", "host", "review")
        .unwrap();
    let h = development_map::handoff::Handoff {
        node_id: "m".into(),
        expected_owner: Some("worker".into()),
        expected_generation: 1,
        new_owner: "next".into(),
        source_ref: "handoff".into(),
    };
    assert_eq!(s.handoff(&h, None).unwrap(), 2);
    assert!(s.handoff(&h, None).is_err());
    assert_eq!(
        s.read(|tx| navigation::focus(tx, "p", None, "dev:m"))
            .unwrap()["node"]["acceptance_state"],
        "stale"
    );
}
#[test]
fn full_ancestor_pagination_detects_midflight_changes() {
    let (_d, s) = setup();
    s.node(&node("a", None)).unwrap();
    s.node(&node("m", Some("a"))).unwrap();
    let p = s
        .read(|tx| navigation::ancestors(tx, "p", "v1", "dev:m", None, 1))
        .unwrap();
    let after = p["next"].as_str().unwrap();
    assert_eq!(
        s.read(|tx| navigation::ancestors(tx, "p", "v1", "dev:m", Some(after), 100))
            .unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    s.node(&node("m", None)).unwrap();
    assert!(s
        .read(|tx| navigation::ancestors(tx, "p", "v1", "dev:m", Some(after), 100))
        .is_err());
}
#[test]
fn deep_navigation_returns_complete_paginated_breadcrumbs() {
    let (_d, s) = setup();
    let mut db = s.connect(false).unwrap();
    let tx = db.transaction().unwrap();
    for i in 0..130 {
        let parent = if i == 0 {
            None
        } else {
            Some(format!("d{}", i - 1))
        };
        tx.execute("INSERT INTO map_dev_nodes(id,project_id,parent_id,kind,title,goal,sort_key,plan_state,updated_at) VALUES(?,'p',?,'module',?,'',?,'planned',0)",rusqlite::params![format!("d{i}"),parent,format!("Depth {i}"),format!("{i:03}")]).unwrap();
    }
    tx.commit().unwrap();
    s.rebuild_projections().unwrap();
    let f = s
        .read(|tx| navigation::focus(tx, "p", None, "dev:d129"))
        .unwrap();
    assert_eq!(f["breadcrumbs"].as_array().unwrap().len(), 100);
    let rest = s
        .read(|tx| {
            navigation::ancestors(tx, "p", "v1", "dev:d129", f["ancestors_next"].as_str(), 100)
        })
        .unwrap();
    assert_eq!(rest["items"].as_array().unwrap().len(), 32);
    assert_eq!(
        rest["items"].as_array().unwrap().last().unwrap()["node_ref"],
        "project:p"
    );
    assert!(rest["next"].is_null());
}
