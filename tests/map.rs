mod support;
use development_map::local_source::{
    semantic_digest, Link as LocalLink, Node as LocalNode, Snapshot,
};
use development_map::store::{self, ArtifactInput, CheckpointInput, MapStore, NodeInput};
use serde_json::{json, Map};
use tempfile::tempdir;
fn setup() -> (tempfile::TempDir, MapStore) {
    let d = tempdir().unwrap();
    let s = MapStore::new(d.path().join("map.sqlite"));
    s.migrate().unwrap();
    s.project("p", "Project", "plan@v1").unwrap();
    s.adopt_architecture("p", "v1", "architecture.md", "1234567890123456")
        .unwrap();
    (d, s)
}
fn node(id: &str, parent: Option<&str>, owner: Option<&str>) -> NodeInput {
    NodeInput {
        id: id.into(),
        project_id: "p".into(),
        parent_id: parent.map(str::to_string),
        kind: "module".into(),
        title: id.into(),
        goal: "goal".into(),
        sort_key: id.into(),
        plan_state: "active".into(),
        owner: owner.map(str::to_string),
        generation: 1,
        architecture_ref: Some("design@v1".into()),
    }
}
fn checkpoint(version: i64, digest: &str) -> CheckpointInput {
    CheckpointInput {
        node_id: "m".into(),
        owner: "worker".into(),
        generation: 1,
        version,
        digest: digest.into(),
        summary: "host tests".into(),
        source_ref: "checkpoint.json".into(),
        git_revision: Some("abc".into()),
    }
}

fn local_snapshot(observed_at: i64, title: &str) -> Snapshot {
    let mut snapshot = Snapshot {
        project_id: "p".into(),
        name: "Project".into(),
        core_node: "p/core".into(),
        source_root: "/fixture".into(),
        config_digest: "1234567890123456".into(),
        digest: String::new(),
        observed_at,
        stale_after_seconds: 900,
        nodes: vec![LocalNode {
            id: "p/core".into(),
            parent_id: None,
            title: title.into(),
            kind: "domain".into(),
            details: json!({"source_status":"active","source_ref":"ledger#core"}),
        }],
        files: vec![],
        links: Vec::<LocalLink>::new(),
        metadata: Map::new(),
    };
    snapshot.digest = semantic_digest(&snapshot).unwrap();
    snapshot
}

#[test]
fn adopted_project_keeps_read_only_source_and_managed_subtree_separate() {
    let d = tempdir().unwrap();
    let s = MapStore::new(d.path().join("map.sqlite"));
    s.migrate().unwrap();
    s.import_local_source(&local_snapshot(1, "Imported core"))
        .unwrap();
    s.adopt_architecture("p", "v1", "architecture.json", "1234567890123456")
        .unwrap();
    let managed = NodeInput {
        id: "p/native/linux".into(),
        project_id: "p".into(),
        parent_id: Some("p/core".into()),
        kind: "module".into(),
        title: "Linux".into(),
        goal: "Submit native checkpoints".into(),
        sort_key: "linux".into(),
        plan_state: "active".into(),
        owner: Some("worker".into()),
        generation: 1,
        architecture_ref: Some("architecture.json#linux".into()),
    };
    s.node(&managed).unwrap();
    s.import_local_source(&local_snapshot(2, "Imported core refreshed"))
        .unwrap();
    s.read(|tx| {
        let (owner, generation, title): (Option<String>, i64, String) = tx.query_row(
            "SELECT owner,generation,title FROM map_dev_nodes WHERE id='p/native/linux'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(owner.as_deref(), Some("worker"));
        assert_eq!(generation, 1);
        assert_eq!(title, "Linux");
        let local_title: String = tx.query_row(
            "SELECT title FROM map_dev_nodes WHERE id='p/core'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(local_title, "Imported core refreshed");
        Ok(())
    })
    .unwrap();
    assert!(s
        .submit(&development_map::submission::Submission {
            base_checkpoint_rowid: None,
            submission_key: "managed-g1-v1".into(),
            checkpoint: CheckpointInput {
                node_id: managed.id.clone(),
                owner: "worker".into(),
                generation: 1,
                version: 1,
                digest: "abcdefabcdefabcd".into(),
                summary: "Managed result".into(),
                source_ref: "checkpoint.json".into(),
                git_revision: None,
            },
            artifacts: vec![],
            resources: None,
        })
        .is_ok());
}
#[test]
fn tree_cycle_and_project_boundary() {
    let (_d, s) = setup();
    s.node(&node("m", None, Some("worker"))).unwrap();
    s.node(&node("child", Some("m"), None)).unwrap();
    assert!(s.node(&node("m", Some("child"), Some("worker"))).is_err());
    s.project("other", "Other", "").unwrap();
    let mut n = node("cross", Some("m"), None);
    n.project_id = "other".into();
    assert!(s.node(&n).is_err());
    s.dependency("child", "m").unwrap();
    assert!(s.dependency("m", "child").is_err());
}
#[test]
fn checkpoint_acceptance_and_restart() {
    let (_d, s) = setup();
    s.node(&node("m", None, Some("worker"))).unwrap();
    let submit = |checkpoint: &CheckpointInput| {
        s.submit(&development_map::submission::Submission {
            base_checkpoint_rowid: None,
            submission_key: format!("logical:{}", checkpoint.version),
            checkpoint: checkpoint.clone(),
            artifacts: vec![],
            resources: Some(support::logical_manifest()),
        })
        .map(|r| r.created)
    };
    assert!(submit(&checkpoint(1, "1234567890123456")).unwrap());
    assert!(!submit(&checkpoint(1, "1234567890123456")).unwrap());
    assert!(submit(&checkpoint(1, "differentdigest123")).is_err());
    s.accept("m", 1, 1, "1234567890123456", "accepted", "host", "review")
        .unwrap();
    let accepted = s.read(|tx| store::node_detail(tx, "m")).unwrap();
    assert_eq!(accepted["acceptance_state"], "accepted");
    submit(&checkpoint(2, "abcdefabcdefabcdef")).unwrap();
    let reopened = MapStore::new(s.connect(true).unwrap().path().unwrap());
    let stale = reopened.read(|tx| store::node_detail(tx, "m")).unwrap();
    assert_eq!(stale["acceptance_state"], "stale");
}
#[test]
fn artifact_shared_reverse_and_pages() {
    let (_d, s) = setup();
    s.node(&node("m", None, Some("worker"))).unwrap();
    s.node(&node("n", None, None)).unwrap();
    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .arg(_d.path())
        .status()
        .unwrap()
        .success());
    s.register_repository("repo", _d.path(), "test").unwrap();
    s.register_project_repository("p", "repo").unwrap();
    for id in ["m", "n"] {
        s.artifact(&ArtifactInput {
            node_id: id.into(),
            repo_key: "repo".into(),
            revision: "abc".into(),
            path: "src/lib.rs".into(),
            kind: "file".into(),
            provenance: "declared".into(),
            verification: "unverified".into(),
            content_digest: None,
        })
        .unwrap()
    }
    s.artifact(&ArtifactInput {
        node_id: "m".into(),
        repo_key: "repo".into(),
        revision: "abc".into(),
        path: "src".into(),
        kind: "directory".into(),
        provenance: "observed".into(),
        verification: "unverified".into(),
        content_digest: None,
    })
    .unwrap();
    let v = s
        .read(|tx| store::lookup(tx, "repo", "abc", "src/lib.rs", 10))
        .unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 3);
    let p = s
        .read(|tx| store::children(tx, "project:p", "", 1, None))
        .unwrap();
    assert!(p["next"].is_string());
}

#[test]
fn architecture_trace_and_recursive_submodule() {
    let (_d, s) = setup();
    s.architecture_node(
        "a",
        "p",
        None,
        "System",
        "system",
        "architecture.md#system",
        "0",
    )
    .unwrap();
    s.architecture_node(
        "a1",
        "p",
        Some("a"),
        "Flow",
        "function",
        "architecture.md#flow",
        "1",
    )
    .unwrap();
    s.node(&node("m", None, Some("worker"))).unwrap();
    let mut sub = node("sub", Some("m"), None);
    sub.kind = "submodule".into();
    s.node(&sub).unwrap();
    let mut f = node("f", Some("sub"), None);
    f.kind = "function".into();
    s.node(&f).unwrap();
    s.trace("a1", "f", "implements", "trace.md").unwrap();
    let v = s.read(|tx| store::node_detail(tx, "f")).unwrap();
    assert_eq!(v["architecture"]["version"], "v1");
    assert_eq!(v["architecture_traces"][0]["id"], "a1");
    let c = s
        .read(|tx| store::architecture_children(tx, "p", Some("a"), "", 50))
        .unwrap();
    assert_eq!(c["items"][0]["id"], "a1");
}
#[test]
fn legacy_bridge_projection_is_read_only_and_idempotent() {
    let d = tempdir().unwrap();
    let path = d.path().join("bridge.sqlite");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE projects(id TEXT PRIMARY KEY,name TEXT,source_ref TEXT,updated_at REAL);CREATE TABLE project_nodes(id TEXT PRIMARY KEY,project_id TEXT,parent_id TEXT,title TEXT,goal TEXT,updated_at REAL);INSERT INTO projects VALUES('old','Old','source',1);INSERT INTO project_nodes VALUES('child','old','parent','Child','goal',1);INSERT INTO project_nodes VALUES('parent','old',NULL,'Parent','goal',1);").unwrap();
    drop(c);
    let s = MapStore::new(&path);
    s.migrate().unwrap();
    assert_eq!(s.import_legacy().unwrap(), 2);
    assert_eq!(s.import_legacy().unwrap(), 0);
    let c = s.connect(true).unwrap();
    let original: String = c
        .query_row(
            "SELECT title FROM project_nodes WHERE id='child'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(original, "Child");
}
#[test]
fn architecture_version_change_hides_old_links() {
    let (_d, s) = setup();
    s.architecture_node("a", "p", None, "System", "system", "design#s", "0")
        .unwrap();
    s.node(&node("m", None, Some("worker"))).unwrap();
    s.trace("a", "m", "primary", "trace").unwrap();
    s.adopt_architecture("p", "v2", "design2", "abcdefabcdefabcd")
        .unwrap();
    let v = s.read(|tx| store::node_detail(tx, "m")).unwrap();
    assert_eq!(v["architecture_traces"].as_array().unwrap().len(), 0);
    assert!(s.trace("a", "m", "implements", "trace").is_err());
}
#[test]
fn scoped_search_and_navigation() {
    let (_d, s) = setup();
    s.architecture_node("a", "p", None, "System", "system", "design", "0")
        .unwrap();
    s.node(&node("m", None, Some("worker"))).unwrap();
    s.trace("a", "m", "primary", "trace").unwrap();
    let q = s.read(|tx| store::search(tx, "p", "m", 10));
    assert!(q.is_err());
    let q = s.read(|tx| store::search(tx, "p", "mo", 10)).unwrap();
    assert_eq!(q["items"].as_array().unwrap().len(), 0);
    let n = s
        .read(|tx| store::navigation_children(tx, "project:p", "", 10))
        .unwrap();
    assert_eq!(n["items"][0]["kind"], "architecture");
    let a = s
        .read(|tx| store::navigation_children(tx, "arch:a", "", 10))
        .unwrap();
    assert_eq!(a["items"][0]["node_ref"], "dev:m");
}
#[test]
fn concurrent_checkpoint_retries_are_idempotent() {
    let (_d, s) = setup();
    s.node(&node("m", None, Some("worker"))).unwrap();
    let mut handles = Vec::new();
    for _ in 0..8 {
        let copy = s.clone();
        handles.push(std::thread::spawn(move || {
            copy.checkpoint(&checkpoint(1, "1234567890123456"))
        }));
    }
    let mut inserted = 0;
    for h in handles {
        if h.join().unwrap().unwrap() {
            inserted += 1
        }
    }
    assert_eq!(inserted, 1);
    let v = s.read(|tx| store::checkpoints(tx, "m", 0, 50)).unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
}
#[test]
fn git_verified_artifact_requires_real_object() {
    use std::process::Command;
    let (_d, s) = setup();
    s.node(&node("m", None, Some("worker"))).unwrap();
    let repo = tempdir().unwrap();
    let run = |args: &[&str]| {
        let o = Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    run(&["init", "-q"]);
    std::fs::write(repo.path().join("code.rs"), "fn main() {}\n").unwrap();
    run(&["add", "code.rs"]);
    run(&[
        "-c",
        "user.name=Map Test",
        "-c",
        "user.email=map@example.invalid",
        "commit",
        "-qm",
        "fixture",
    ]);
    let sha = run(&["rev-parse", "HEAD"]);
    let a = ArtifactInput {
        node_id: "m".into(),
        repo_key: "fixture".into(),
        revision: sha,
        path: "code.rs".into(),
        kind: "file".into(),
        provenance: "declared".into(),
        verification: "verified".into(),
        content_digest: None,
    };
    assert!(s.artifact(&a).is_err());
    s.register_repository("fixture", repo.path(), "test")
        .unwrap();
    s.register_project_repository("p", "fixture").unwrap();
    assert!(s.artifact_checked(&a, _d.path()).is_err());
    s.artifact_checked(&a, repo.path()).unwrap();
    let mut bad = a.clone();
    bad.path = "missing.rs".into();
    assert!(s.artifact_checked(&bad, repo.path()).is_err());
    let v = s.read(|tx| store::artifacts(tx, "m", 0, 50, None)).unwrap();
    assert_eq!(v["items"][0]["verification"], "verified");
}
#[test]
fn old_checkpoint_cannot_be_accepted_after_new_commit() {
    let (_d, s) = setup();
    s.node(&node("m", None, Some("worker"))).unwrap();
    s.checkpoint(&checkpoint(1, "1234567890123456")).unwrap();
    s.checkpoint(&checkpoint(2, "abcdefabcdefabcd")).unwrap();
    assert!(s
        .accept("m", 1, 1, "1234567890123456", "accepted", "host", "review")
        .is_err());
}
#[test]
fn architecture_coverage_and_interface_scope() {
    let (_d, s) = setup();
    s.architecture_node_full(
        "a",
        "p",
        None,
        "Required",
        "component",
        "design#a",
        "0",
        Some("REQ-1"),
        true,
    )
    .unwrap();
    s.architecture_node_full(
        "b",
        "p",
        None,
        "Optional",
        "component",
        "design#b",
        "1",
        None,
        false,
    )
    .unwrap();
    let v = s.read(|tx| store::architecture(tx, "p")).unwrap();
    assert_eq!(v["uncovered_count"], 1);
    s.architecture_edge("a", "b", "interface", "design#wire")
        .unwrap();
    let n = s
        .read(|tx| store::architecture_neighborhood(tx, "a", 10))
        .unwrap();
    assert_eq!(n["interfaces"][0]["to_id"], "b");
    assert_eq!(n["requirement_id"], "REQ-1");
    s.node(&node("m", None, Some("worker"))).unwrap();
    s.trace("a", "m", "implements", "trace").unwrap();
    let v = s.read(|tx| store::architecture(tx, "p")).unwrap();
    assert_eq!(v["uncovered_count"], 0);
}
#[test]
fn same_architecture_id_preserves_two_revisions_and_links() {
    let (_d, s) = setup();
    s.architecture_node("a", "p", None, "Old title", "component", "design-v1#a", "0")
        .unwrap();
    s.node(&node("m", None, Some("worker"))).unwrap();
    s.trace("a", "m", "implements", "trace-v1").unwrap();
    s.adopt_architecture("p", "v2", "design-v2", "abcdefabcdefabcd")
        .unwrap();
    s.architecture_node("a", "p", None, "New title", "component", "design-v2#a", "0")
        .unwrap();
    let old = s
        .read(|tx| store::architecture_children_revision(tx, "p", None, "", 50, Some("v1")))
        .unwrap();
    let new = s
        .read(|tx| store::architecture_children(tx, "p", None, "", 50))
        .unwrap();
    assert_eq!(old["items"][0]["title"], "Old title");
    assert_eq!(new["items"][0]["title"], "New title");
    let old_trace = s
        .read(|tx| store::architecture_trace_revision(tx, "a", 50, Some("v1")))
        .unwrap();
    let new_trace = s.read(|tx| store::architecture_trace(tx, "a", 50)).unwrap();
    assert_eq!(old_trace["items"].as_array().unwrap().len(), 1);
    assert_eq!(new_trace["items"].as_array().unwrap().len(), 0);
    s.trace("a", "m", "implements", "trace-v2").unwrap();
    let new_trace = s.read(|tx| store::architecture_trace(tx, "a", 50)).unwrap();
    assert_eq!(new_trace["items"].as_array().unwrap().len(), 1);
}
