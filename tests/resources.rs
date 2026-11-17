use development_map::{
    resources::{self, DatabaseRef, Manifest, Resource},
    store::{CheckpointInput, MapStore, NodeInput},
    submission::Submission,
};
use std::{path::Path, process::Command};
use tempfile::{tempdir, TempDir};
fn git(root: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{:?}", o);
    String::from_utf8(o.stdout).unwrap().trim().into()
}
// Resource verification reads Git objects. Build mode-120000 fixtures directly
// so Windows exercises the same rejection without OS symlink privileges.
fn index_symlink(root: &Path, path: &str, target: &str) {
    let fixture = root.join("symlink-target.fixture");
    std::fs::write(&fixture, target).unwrap();
    let blob = git(root, &["hash-object", "-w", "symlink-target.fixture"]);
    std::fs::remove_file(fixture).unwrap();
    git(
        root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{blob},{path}"),
        ],
    );
}
fn node(s: &MapStore, id: &str, parent: Option<&str>) {
    s.node(&NodeInput {
        id: id.into(),
        project_id: "p".into(),
        parent_id: parent.map(str::to_string),
        kind: "module".into(),
        title: id.into(),
        goal: "resource test".into(),
        sort_key: id.into(),
        plan_state: "active".into(),
        owner: Some("worker".into()),
        generation: 1,
        architecture_ref: None,
    })
    .unwrap();
}
fn setup() -> (TempDir, MapStore, String) {
    let d = tempdir().unwrap();
    git(d.path(), &["init", "-q"]);
    git(d.path(), &["config", "user.email", "test@example.invalid"]);
    git(d.path(), &["config", "user.name", "Test"]);
    std::fs::create_dir_all(d.path().join("src/sub")).unwrap();
    for p in [
        "src/a.rs",
        "src/README.md",
        "src/cp.md",
        "src/schema.sql",
        "src/sub/b.rs",
    ] {
        std::fs::write(d.path().join(p), p).unwrap();
    }
    git(d.path(), &["add", "src"]);
    git(d.path(), &["commit", "-qm", "fixture"]);
    let rev = git(d.path(), &["rev-parse", "HEAD"]);
    let s = MapStore::new(d.path().join("db.sqlite"));
    s.migrate().unwrap();
    s.project("p", "Project", "test").unwrap();
    s.adopt_architecture("p", "v1", "test", "1234567890123456")
        .unwrap();
    node(&s, "m", None);
    s.register_repository("r", d.path(), "test").unwrap();
    s.register_project_repository("p", "r").unwrap();
    (d, s, rev)
}
fn resource(category: &str, path: &str, rev: &str) -> Resource {
    Resource {
        key: category.into(),
        category: category.into(),
        repo_key: Some("r".into()),
        revision: Some(rev.into()),
        path: Some(path.into()),
        ownership: "primary".into(),
        required: true,
        not_applicable: false,
        reason: "".into(),
        database: if category == "database" {
            Some(DatabaseRef {
                schema: "main".into(),
                entity: "map_dev_nodes".into(),
                record: "m".into(),
            })
        } else {
            None
        },
    }
}
fn bundle(rev: &str) -> Submission {
    Submission {
        base_checkpoint_rowid: None,
        submission_key: "s1".into(),
        checkpoint: CheckpointInput {
            node_id: "m".into(),
            owner: "worker".into(),
            generation: 1,
            version: 1,
            digest: "1234567890123456".into(),
            summary: "tested paths".into(),
            source_ref: "src/cp.md".into(),
            git_revision: Some(rev.into()),
        },
        artifacts: vec![],
        resources: Some(Manifest {
            architecture_revision: "v1".into(),
            hierarchy_exception: "".into(),
            entries: vec![
                resource("directory", "src", rev),
                resource("code", "src/a.rs", rev),
                resource("document", "src/README.md", rev),
                resource("database", "src/schema.sql", rev),
                resource("checkpoint", "src/cp.md", rev),
            ],
        }),
    }
}
fn read(s: &MapStore, n: &str) -> serde_json::Value {
    s.read(|tx| resources::manifest(tx, n, None)).unwrap()
}
#[test]
fn atomic_exact_idempotent_persistent_and_acceptance() {
    let (_d, s, rev) = setup();
    let mut b = bundle(&rev);
    let r = s.submit(&b).unwrap();
    assert!(!s.submit(&b).unwrap().created);
    assert_eq!(read(&s, "m")["complete"], true);
    let count: i64 = s
        .connect(true)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM map_checkpoint_artifacts WHERE checkpoint_rowid=?",
            [r.checkpoint_rowid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 5);
    s.accept("m", 1, 1, &b.checkpoint.digest, "accepted", "host", "test")
        .unwrap();
    b.submission_key = "s2".into();
    b.checkpoint.version = 2;
    b.resources.as_mut().unwrap().entries[2].path = Some("src/missing.md".into());
    s.submit(&b).unwrap();
    assert_eq!(read(&s, "m")["complete"], false);
    assert!(s
        .accept("m", 1, 2, &b.checkpoint.digest, "accepted", "host", "test")
        .is_err());
    assert_eq!(
        s.read(|tx| resources::manifest(tx, "m", Some(r.checkpoint_rowid)))
            .unwrap()["complete"],
        true
    );
    s.rebuild_projections().unwrap();
    assert_eq!(read(&s, "m")["checkpoint"]["version"], 2);
    b.submission_key = "bad".into();
    b.checkpoint.version = 3;
    b.resources.as_mut().unwrap().entries[0].path = Some("../secret".into());
    assert!(s.submit(&b).is_err());
    assert_eq!(read(&s, "m")["checkpoint"]["version"], 2);
}
#[test]
fn recursive_shared_reverse_pagination_and_hierarchy() {
    let (_d, s, rev) = setup();
    s.submit(&bundle(&rev)).unwrap();
    node(&s, "child", Some("m"));
    let mut b = bundle(&rev);
    b.submission_key = "child".into();
    b.checkpoint.node_id = "child".into();
    let m = b.resources.as_mut().unwrap();
    m.entries[3].database.as_mut().unwrap().record = "child".into();
    m.entries[0].path = Some("src/sub".into());
    for e in &mut m.entries[1..] {
        e.ownership = "shared".into();
    }
    s.submit(&b).unwrap();
    assert_eq!(read(&s, "child")["complete"], true);
    let first = s
        .read(|tx| resources::lookup(tx, "p", "r", &rev, "src/a.rs", "", 1))
        .unwrap();
    assert_eq!(first["items"][0], "child");
    let second = s
        .read(|tx| {
            resources::lookup(
                tx,
                "p",
                "r",
                &rev,
                "src/a.rs",
                first["next"].as_str().unwrap(),
                1,
            )
        })
        .unwrap();
    assert_eq!(second["items"][0], "m");
    let mut parent = bundle(&rev);
    parent.submission_key = "parent-unmapped".into();
    parent.checkpoint.version = 2;
    parent.resources = None;
    s.submit(&parent).unwrap();
    assert_eq!(read(&s, "child")["hierarchy_state"], "pending");
    parent.submission_key = "parent-restored".into();
    parent.checkpoint.version = 3;
    parent.resources = bundle(&rev).resources;
    s.submit(&parent).unwrap();
    assert_eq!(read(&s, "child")["complete"], true);
    b.submission_key = "outside".into();
    b.checkpoint.version = 2;
    b.resources.as_mut().unwrap().entries[0].path = Some("other".into());
    assert!(s.submit(&b).is_err());
    b.resources.as_mut().unwrap().hierarchy_exception = "cross-module architecture package".into();
    s.submit(&b).unwrap();
    assert_eq!(read(&s, "child")["hierarchy_state"], "exception");
}
#[test]
fn rename_versions_deletion_and_history() {
    let (d, s, rev) = setup();
    let first = s.submit(&bundle(&rev)).unwrap();
    git(d.path(), &["mv", "src/a.rs", "src/renamed.rs"]);
    git(d.path(), &["commit", "-qm", "rename"]);
    let next = git(d.path(), &["rev-parse", "HEAD"]);
    let mut b = bundle(&next);
    b.submission_key = "rename".into();
    b.checkpoint.version = 2;
    b.resources.as_mut().unwrap().entries[1].path = Some("src/renamed.rs".into());
    s.submit(&b).unwrap();
    assert_eq!(read(&s, "m")["complete"], true);
    assert!(read(&s, "m")["binding_changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["key"] == "code"
            && c["before"]["path"] == "src/a.rs"
            && c["after"]["path"] == "src/renamed.rs"));
    assert_eq!(
        s.read(|tx| resources::manifest(tx, "m", Some(first.checkpoint_rowid)))
            .unwrap()["entries"][1]["path"],
        "src/a.rs"
    );
    b.submission_key = "mismatch".into();
    b.checkpoint.version = 3;
    b.resources.as_mut().unwrap().entries[1].revision = Some(rev);
    s.submit(&b).unwrap();
    assert_eq!(read(&s, "m")["entries"][1]["status"], "version_mismatch");
    git(d.path(), &["rm", "src/renamed.rs"]);
    git(d.path(), &["commit", "-qm", "delete"]);
    let deleted = git(d.path(), &["rev-parse", "HEAD"]);
    b.checkpoint.git_revision = Some(deleted.clone());
    b.checkpoint.version = 4;
    b.submission_key = "delete".into();
    for e in &mut b.resources.as_mut().unwrap().entries {
        e.revision = Some(deleted.clone());
    }
    s.submit(&b).unwrap();
    assert_eq!(read(&s, "m")["entries"][1]["status"], "missing");
}
#[test]
fn traversal_symlink_cross_project_and_wrong_checkpoint() {
    let (d, s, rev) = setup();
    assert!(s
        .register_repository("nested", &d.path().join("src"), "test")
        .is_err());
    let mut b = bundle(&rev);
    for p in [
        "/etc/passwd",
        "../secret",
        "src/../../secret",
        "src//a",
        "src/./a",
        "src\\a",
    ] {
        b.resources.as_mut().unwrap().entries[1].path = Some(p.into());
        assert!(s.submit(&b).is_err(), "{p}");
    }
    index_symlink(d.path(), "escape", "/etc");
    git(d.path(), &["commit", "-qm", "directory link fixture"]);
    b = bundle(&git(d.path(), &["rev-parse", "HEAD"]));
    b.resources.as_mut().unwrap().entries[1].path = Some("escape/passwd".into());
    assert!(s.submit(&b).is_err());
    b = bundle(&rev);
    b.resources.as_mut().unwrap().entries[0].repo_key = Some("unregistered".into());
    assert!(s.submit(&b).is_err());
    let receipt = s.submit(&bundle(&rev)).unwrap();
    node(&s, "other", None);
    assert!(s
        .read(|tx| resources::manifest(tx, "other", Some(receipt.checkpoint_rowid)))
        .is_err());
    assert!(s
        .read(|tx| resources::directory(tx, "m", receipt.checkpoint_rowid, "directory", "", "", 30))
        .is_err());
    let page = s
        .read(|tx| {
            resources::directory(tx, "m", receipt.checkpoint_rowid, "directory", "src", "", 2)
        })
        .unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    assert!(page["next"].is_string());
}
#[test]
fn legacy_migration_and_explicit_not_applicable() {
    let (_d, s, rev) = setup();
    let mut b = bundle(&rev);
    b.resources = None;
    s.submit(&b).unwrap();
    assert_eq!(read(&s, "m")["mapping_state"], "missing");
    assert!(s
        .accept("m", 1, 1, &b.checkpoint.digest, "accepted", "host", "test")
        .is_err());
    // Simulate a v4 database whose resource tables have not yet been created.
    let db = s.connect(false).unwrap();
    db.execute_batch("DROP TABLE map_resource_entries; DROP TABLE map_resource_sets; DROP TABLE map_project_repositories; DELETE FROM map_schema_migrations WHERE version>=5; INSERT OR IGNORE INTO map_schema_migrations VALUES(4,unixepoch());").unwrap();
    drop(db);
    s.migrate().unwrap();
    assert!(s
        .accept("m", 1, 1, &b.checkpoint.digest, "accepted", "host", "test")
        .is_err());
    s.register_project_repository("p", "r").unwrap();
    assert_eq!(read(&s, "m")["checkpoint"]["version"], 1);
    b = bundle(&rev);
    b.submission_key = "na".into();
    b.checkpoint.version = 2;
    let e = &mut b.resources.as_mut().unwrap().entries[3];
    e.not_applicable = true;
    e.repo_key = None;
    e.path = None;
    e.revision = None;
    e.database = None;
    e.reason = "No business database; map record is authoritative".into();
    s.submit(&b).unwrap();
    assert_eq!(read(&s, "m")["entries"][3]["status"], "not_applicable");
}

#[test]
fn database_claims_and_git_symlink_cannot_fake_verification() {
    let (d, s, rev) = setup();
    let mut b = bundle(&rev);
    b.resources.as_mut().unwrap().entries[3]
        .database
        .as_mut()
        .unwrap()
        .record = "someone-else".into();
    assert!(s.submit(&b).is_err());
    b = bundle(&rev);
    b.resources.as_mut().unwrap().entries[3]
        .database
        .as_mut()
        .unwrap()
        .entity = "external_orders".into();
    s.submit(&b).unwrap();
    assert_eq!(read(&s, "m")["entries"][3]["status"], "pending");
    assert!(s
        .accept("m", 1, 1, &b.checkpoint.digest, "accepted", "host", "test")
        .is_err());
    index_symlink(d.path(), "src/link", "/etc/passwd");
    git(d.path(), &["commit", "-qm", "link fixture"]);
    let next = git(d.path(), &["rev-parse", "HEAD"]);
    b = bundle(&next);
    b.checkpoint.version = 2;
    b.submission_key = "symlink".into();
    b.resources.as_mut().unwrap().entries[1].path = Some("src/link".into());
    assert!(s.submit(&b).is_err());
    let mut encoded = serde_json::to_value(bundle(&rev)).unwrap();
    encoded["resources"]["entries"][0]["status"] = serde_json::json!("verified");
    assert!(serde_json::from_value::<Submission>(encoded).is_err());
}

#[test]
fn manifest_pages_are_snapshot_scoped() {
    let (_d, s, rev) = setup();
    let mut b = bundle(&rev);
    for i in 0..22 {
        let mut e = resource("evidence", "src/a.rs", &rev);
        e.key = format!("evidence-{i}");
        e.required = false;
        e.ownership = "shared".into();
        b.resources.as_mut().unwrap().entries.push(e);
    }
    let receipt = s.submit(&b).unwrap();
    let first = s.read(|tx| resources::page(tx, "m", None, "", 20)).unwrap();
    assert_eq!(first["entries"].as_array().unwrap().len(), 20);
    let cursor = first["next"].as_str().unwrap();
    let last = s
        .read(|tx| resources::page(tx, "m", None, cursor, 20))
        .unwrap();
    assert_eq!(last["entries"].as_array().unwrap().len(), 7);
    b.submission_key = "next-page-version".into();
    b.checkpoint.version = 2;
    s.submit(&b).unwrap();
    assert!(s
        .read(|tx| resources::page(tx, "m", None, cursor, 20))
        .is_err());
    assert!(s
        .read(|tx| resources::page(tx, "m", Some(receipt.checkpoint_rowid), cursor, 20))
        .is_ok());
}

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use development_map::identity::Binding;
use tower::ServiceExt;
fn bind(s: &MapStore, token: &str, project: &str, role: &str, node: Option<&str>, exp: i64) {
    s.bind_identity(
        token,
        &Binding {
            subject: format!("{role}-{project}"),
            project_id: project.into(),
            task_id: format!("{role}-{project}"),
            owner: role.into(),
            role: role.into(),
            node_id: node.map(str::to_string),
            generation: node.map(|_| 1),
            source_ref: "review".into(),
            expires_at: exp,
        },
    )
    .unwrap();
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
) -> (StatusCode, usize, Vec<u8>) {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    let r = app
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let st = r.status();
    let bytes = axum::body::to_bytes(r.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (st, bytes.len(), bytes)
}
#[tokio::test]
async fn populated_resource_and_snapshot_http() {
    let (_d, s, rev) = setup();
    let cp = s.submit(&bundle(&rev)).unwrap().checkpoint_rowid;
    let mut snap:development_map::local_source::Snapshot=serde_json::from_value(serde_json::json!({"project_id":"p","name":"p","core_node":"p/core","source_root":"fixture","config_digest":"fixture","digest":"","observed_at":1,"stale_after_seconds":900,"nodes":[{"id":"p/core","parent_id":null,"title":"snapshot core","kind":"domain","details":{}}],"files":[{"path":"src","kind":"directory"},{"path":"src/a.rs","kind":"file"}],"links":[{"node_id":"p/core","path":"src/a.rs","category":"code"}]})).unwrap();
    snap.digest = development_map::local_source::semantic_digest(&snap).unwrap();
    s.import_local_source(&snap).unwrap();
    s.project("q", "q", "fixture").unwrap();
    let own = "fixture-reader-p-01234567890123456789";
    let other = "fixture-reader-q-01234567890123456789";
    bind(&s, own, "p", "reader", None, 4102444800);
    bind(&s, other, "q", "reader", None, 4102444800);
    let app = development_map::web::MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    for (uri, needle, denied) in [
        (
            format!("/v1/map/nodes/m/directory?key=directory&path=src&checkpoint_id={cp}"),
            "a.rs",
            404,
        ),
        (
            "/v1/map/local-source/file?project_id=p&path=src/a.rs".into(),
            "src/a.rs",
            403,
        ),
        (
            "/v1/map/local-source/tree?project_id=p&path=src".into(),
            "src/a.rs",
            403,
        ),
        (
            "/v1/map/local-source/lookup?project_id=p&path=src/a.rs".into(),
            "p/core",
            403,
        ),
    ] {
        let (st, _, body) = call(&app, "GET", &uri, Some(own)).await;
        assert_eq!(st, StatusCode::OK, "{uri}");
        assert!(String::from_utf8(body).unwrap().contains(needle));
        let (st, len, _) = call(&app, "GET", &uri, Some(other)).await;
        assert_eq!(st.as_u16(), denied);
        assert_eq!(len, 0);
    }
}
