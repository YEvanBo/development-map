use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use development_map::{store::MapStore, web::MapWeb};
use tempfile::tempdir;
use tower::ServiceExt;
#[tokio::test]
async fn session_is_single_use_and_api_requires_identity() {
    let d = tempdir().unwrap();
    let s = MapStore::new(d.path().join("map.sqlite"));
    s.migrate().unwrap();
    let w = MapWeb::new(s, "long-bearer-token".into());
    let code = w.issue_launch_code();
    let router = w.router();
    let unauth = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/map/projects")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);
    let request = || {
        Request::builder()
            .method("POST")
            .uri("/map/session")
            .header("origin", "http://127.0.0.1:8765")
            .header("host", "127.0.0.1:8765")
            .header("content-type", "application/json")
            .body(Body::from(format!("{{\"code\":\"{code}\"}}")))
            .unwrap()
    };
    let first = router.clone().oneshot(request()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert!(first.headers().get_all("set-cookie").iter().count() == 2);
    let replay = router.clone().oneshot(request()).await.unwrap();
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    let bearer = router
        .oneshot(
            Request::builder()
                .uri("/v1/map/projects")
                .header("authorization", "Bearer long-bearer-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bearer.status(), StatusCode::OK);
}
#[tokio::test]
async fn write_roles_are_separate_from_read_cookie_and_bearer() {
    let d = tempdir().unwrap();
    let s = MapStore::new(d.path().join("map.sqlite"));
    s.migrate().unwrap();
    let w = MapWeb::new(s, "read-token-12345678901234567890".into())
        .with_write_tokens(
            "worker-token-12345678901234567890".into(),
            "manager-token-12345678901234567890".into(),
        )
        .unwrap();
    let r = w.router();
    let body = r#"{"node_id":"missing","owner":"x","generation":1,"version":1,"digest":"1234567890123456","summary":"s","source_ref":"c"}"#;
    let post = |token: &str| {
        Request::builder()
            .method("POST")
            .uri("/v1/map/checkpoints")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(body))
            .unwrap()
    };
    let read = r
        .clone()
        .oneshot(post("read-token-12345678901234567890"))
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::FORBIDDEN);
    let manager = r
        .clone()
        .oneshot(post("manager-token-12345678901234567890"))
        .await
        .unwrap();
    assert_eq!(manager.status(), StatusCode::FORBIDDEN);
    let worker = r
        .oneshot(post("worker-token-12345678901234567890"))
        .await
        .unwrap();
    assert_eq!(worker.status(), StatusCode::NOT_FOUND);
}
#[tokio::test]
async fn scoped_identity_cannot_read_other_project_or_forge_checkpoint_owner() {
    use development_map::{
        identity::Binding,
        store::{CheckpointInput, NodeInput},
    };
    let d = tempdir().unwrap();
    let s = MapStore::new(d.path().join("map.sqlite"));
    s.migrate().unwrap();
    for p in ["p", "other"] {
        s.project(p, p, "source").unwrap();
        s.adopt_architecture(p, "v1", "design", "1234567890123456")
            .unwrap();
        s.node(&NodeInput {
            id: format!("{p}-node"),
            project_id: p.into(),
            parent_id: None,
            kind: "module".into(),
            title: p.into(),
            goal: "".into(),
            sort_key: p.into(),
            plan_state: "active".into(),
            owner: Some("worker".into()),
            generation: 1,
            architecture_ref: None,
        })
        .unwrap();
    }
    let token = "scoped-worker-identity-token-123456789";
    s.bind_identity(
        token,
        &Binding {
            subject: "account".into(),
            project_id: "p".into(),
            task_id: "task".into(),
            owner: "worker".into(),
            role: "worker".into(),
            node_id: Some("p-node".into()),
            generation: Some(1),
            source_ref: "host".into(),
            expires_at: 4102444800,
        },
    )
    .unwrap();
    let app = MapWeb::new(s.clone(), "old-global-bearer".into())
        .with_scoped_identities()
        .router();
    for (url, status) in [
        ("/v1/map/nodes/p-node", StatusCode::OK),
        ("/v1/map/nodes/other-node", StatusCode::NOT_FOUND),
        (
            "/v1/map/navigation?project_id=other&node_ref=dev:p-node",
            StatusCode::NOT_FOUND,
        ),
        (
            "/v1/map/artifacts/lookup?repo_key=x&revision=y&path=z",
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
        assert_eq!(r.status(), status, "{url}");
    }
    let cp = CheckpointInput {
        node_id: "p-node".into(),
        owner: "forged".into(),
        generation: 1,
        version: 1,
        digest: "1234567890123456".into(),
        summary: "".into(),
        source_ref: "cp".into(),
        git_revision: None,
    };
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/map/checkpoints")
                .method("POST")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&cp).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    s.revoke_identity(token).unwrap();
    let r = app
        .oneshot(
            Request::builder()
                .uri("/v1/map/nodes/p-node")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}
#[tokio::test]
async fn scoped_unicode_and_slash_node_ids_are_decoded_before_authorization() {
    use development_map::{identity::Binding, store::NodeInput};
    let d = tempdir().unwrap();
    let s = MapStore::new(d.path().join("map.sqlite"));
    s.migrate().unwrap();
    s.project("p", "P", "s").unwrap();
    s.adopt_architecture("p", "v1", "d", "1234567890123456")
        .unwrap();
    s.node(&NodeInput {
        id: "p/节点".into(),
        project_id: "p".into(),
        parent_id: None,
        kind: "module".into(),
        title: "节点".into(),
        goal: "".into(),
        sort_key: "a".into(),
        plan_state: "active".into(),
        owner: None,
        generation: 0,
        architecture_ref: None,
    })
    .unwrap();
    let token = "unicode-reader-token-long-enough-12345";
    s.bind_identity(
        token,
        &Binding {
            subject: "reader".into(),
            project_id: "p".into(),
            task_id: "reader-task".into(),
            owner: "reader".into(),
            role: "reader".into(),
            node_id: None,
            generation: None,
            source_ref: "host".into(),
            expires_at: 4102444800,
        },
    )
    .unwrap();
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    let r = app
        .oneshot(
            Request::builder()
                .uri("/v1/map/nodes/p%2F%E8%8A%82%E7%82%B9")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}
