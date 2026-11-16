use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use development_map::{
    identity::Binding,
    store::{ArtifactInput, MapStore, NodeInput},
    web::MapWeb,
};
use std::process::Command;
use tower::ServiceExt;
fn node(id: &str, project: &str) -> NodeInput {
    NodeInput {
        id: id.into(),
        project_id: project.into(),
        parent_id: None,
        kind: "module".into(),
        title: id.into(),
        goal: "review".into(),
        sort_key: id.into(),
        plan_state: "active".into(),
        owner: Some("worker".into()),
        generation: 1,
        architecture_ref: None,
    }
}
fn setup() -> (tempfile::TempDir, MapStore) {
    let d = tempfile::tempdir().unwrap();
    let s = MapStore::new(d.path().join("map.sqlite"));
    s.migrate().unwrap();
    for p in ["p", "other"] {
        s.project(p, p, "test").unwrap();
        s.adopt_architecture(p, "v1", "design", "1234567890123456")
            .unwrap();
        s.node(&node(&format!("{p}-node"), p)).unwrap();
        s.architecture_node(&format!("{p}-arch"), p, None, p, "component", "design", "1")
            .unwrap();
    }
    (d, s)
}
#[test]
fn dependency_cannot_link_private_project() {
    let (_d, s) = setup();
    assert!(s.dependency("p-node", "other-node").is_err());
}
#[tokio::test]
async fn query_parameters_cannot_override_route_scope() {
    let (_d, s) = setup();
    let token = "review-worker-token-1234567890";
    s.bind_identity(
        token,
        &Binding {
            subject: "review".into(),
            project_id: "p".into(),
            task_id: "task".into(),
            owner: "worker".into(),
            role: "worker".into(),
            node_id: Some("p-node".into()),
            generation: Some(1),
            source_ref: "test".into(),
            expires_at: 4102444800,
        },
    )
    .unwrap();
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    let head = app
        .clone()
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/v1/map/nodes/other-node")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(head.status(), StatusCode::NOT_FOUND);
    for (url, want) in [
        (
            "/v1/map/nodes/other-node/checkpoint-artifacts?checkpoint_id=1&project_id=p",
            StatusCode::NOT_FOUND,
        ),
        (
            "/v1/map/architecture/other-arch/neighborhood?node_id=p-node",
            StatusCode::NOT_FOUND,
        ),
        (
            "/v1/map/children?parent_id=project:other&node_id=p-node",
            StatusCode::NOT_FOUND,
        ),
        (
            "/v1/map/search?project_id=p&node_id=p-node&q=other",
            StatusCode::FORBIDDEN,
        ),
    ] {
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(url)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), want, "{url}");
    }
}
fn git(root: &std::path::Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{:?}", o);
    String::from_utf8(o.stdout).unwrap().trim().into()
}
#[test]
fn deleted_files_emit_impact_without_fabricating_artifacts() {
    let (d, s) = setup();
    let root = d.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "EvanBo"]);
    git(&root, &["config", "user.email", "review@example.invalid"]);
    std::fs::write(root.join("api.rs"), "pub fn api() {}\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "initial"]);
    let from = git(&root, &["rev-parse", "HEAD"]);
    s.register_repository("repo", &root, "test").unwrap();
    s.register_project_repository("p", "repo").unwrap();
    s.artifact(&ArtifactInput {
        node_id: "p-node".into(),
        repo_key: "repo".into(),
        revision: from.clone(),
        path: "api.rs".into(),
        kind: "file".into(),
        provenance: "declared".into(),
        verification: "unverified".into(),
        content_digest: None,
    })
    .unwrap();
    git(&root, &["rm", "-q", "api.rs"]);
    git(&root, &["commit", "-qm", "remove API"]);
    let to = git(&root, &["rev-parse", "HEAD"]);
    s.scan_git("repo", &from, &to).unwrap();
    s.read(|tx|{let count:i64=tx.query_row("SELECT count(*) FROM map_events WHERE node_id='p-node' AND kind='git_observation' AND detail=?",[&to],|r|r.get(0))?;assert_eq!(count,1,"deletion must invalidate affected-node context");let artifacts:i64=tx.query_row("SELECT count(*) FROM map_artifacts WHERE revision=?",[&to],|r|r.get(0))?;assert_eq!(artifacts,0);Ok(())}).unwrap();
}
#[test]
fn unsupported_progress_must_not_be_silently_discarded() {
    let payload = serde_json::json!({"submission_key":"unsupported-progress","checkpoint":{"node_id":"p-node","owner":"worker","generation":1,"version":1,"digest":"1234567890123456","summary":"summary","source_ref":"checkpoint"},"artifacts":[],"progress":{"remaining":{"state":"none"}}});
    assert!(
        serde_json::from_value::<development_map::submission::Submission>(payload).is_err(),
        "unknown progress was accepted but never persisted"
    );
}
#[tokio::test]
async fn empty_legacy_bearer_must_not_disable_authentication() {
    let (_d, s) = setup();
    let app = MapWeb::new(s, String::new()).router();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/map/projects")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
#[test]
fn historical_cross_project_edges_are_not_exposed() {
    let (_d, s) = setup();
    // Deliberately reproduce a legacy row allowed by V1, without deleting history.
    s.connect(false)
        .unwrap()
        .execute(
            "INSERT INTO map_edges VALUES('p-node','other-node','depends_on')",
            [],
        )
        .unwrap();
    s.read(|tx| {
        let page = development_map::relations::dependencies(tx, "p-node", "", 10)?;
        assert!(page["items"].as_array().unwrap().is_empty());
        let node = development_map::store::node_detail(tx, "p-node")?;
        assert!(node["depends_on"].as_array().unwrap().is_empty());
        Ok(())
    })
    .unwrap();
}

#[test]
fn dependency_events_only_track_actual_changes_and_ignore_foreign_tables() {
    let (dir, store) = setup();
    let db = rusqlite::Connection::open(dir.path().join("map.sqlite")).unwrap();
    db.execute("CREATE TABLE events(unrelated TEXT)", [])
        .unwrap();
    store.node(&node("second", "p")).unwrap();
    for _ in 0..2 {
        store.dependency("p-node", "second").unwrap();
    }
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM map_events WHERE kind='dependency' AND node_id='p-node'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn different_project_nodes_can_reuse_submission_key() {
    let (_dir, store) = setup();
    for id in ["p-node", "other-node"] {
        let input = serde_json::from_value(serde_json::json!({
            "submission_key":"checkpoint-1","checkpoint":{
                "node_id":id,"owner":"worker","generation":1,"version":1,
                "digest":"1234567890123456","summary":"verified","source_ref":"test"
            },"artifacts":[]
        }))
        .unwrap();
        assert!(store.submit_bound(&input, None).unwrap().created);
        assert!(!store.submit_bound(&input, None).unwrap().created);
    }
}

fn check_git_replacement(format: &str) {
    use sha2::{Digest, Sha256};
    let (d, s) = setup();
    let root = d.path().join("replace-test");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", &format!("--object-format={format}")]);
    git(&root, &["config", "user.name", "QA"]);
    git(&root, &["config", "user.email", "qa@example.invalid"]);
    std::fs::write(root.join("fact"), "original").unwrap();
    git(&root, &["add", "fact"]);
    git(&root, &["commit", "-qm", "original"]);
    let original = git(&root, &["rev-parse", "HEAD"]);
    std::fs::write(root.join("fact"), "replacement").unwrap();
    std::fs::write(root.join("replacement_only"), "extra").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "replacement"]);
    let replacement = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["replace", &original, &replacement]);
    assert_eq!(
        git(&root, &["cat-file", "blob", &format!("{original}:fact")]),
        "replacement"
    );
    s.register_repository("r", &root, "fixture").unwrap();
    s.register_project_repository("p", "r").unwrap();
    let mut a = ArtifactInput {
        node_id: "p-node".into(),
        repo_key: "r".into(),
        revision: original.clone(),
        path: "fact".into(),
        kind: "file".into(),
        provenance: "observed".into(),
        verification: "verified".into(),
        content_digest: Some(format!("{:x}", Sha256::digest(b"replacement"))),
    };
    assert!(
        s.artifact_checked(&a, &root).is_err(),
        "replacement digest accepted for original"
    );
    a.content_digest = Some(format!("{:x}", Sha256::digest(b"original")));
    s.artifact_checked(&a, &root).unwrap();
    a.path = "replacement_only".into();
    a.content_digest = None;
    assert!(
        s.artifact_checked(&a, &root).is_err(),
        "replacement-only path accepted"
    );
    assert_eq!(s.scan_git("r", &original, &replacement).unwrap(), 2);
    let changes = s.git_changes("r", &original, &replacement, -1, 10).unwrap();
    let paths: Vec<_> = changes["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"fact") && paths.contains(&"replacement_only"));
}
#[test]
fn git_replace_sha1() {
    check_git_replacement("sha1");
}
#[test]
fn git_replace_sha256() {
    check_git_replacement("sha256");
}
