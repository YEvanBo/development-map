// DEVMap-V1-PREOPEN-20261115-A independent route/role matrix (review regression).
// Not part of the original candidate. Added by the independent reviewer.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use development_map::{
    agent::Resume,
    identity::Binding,
    store::{MapStore, NodeInput},
    web::MapWeb,
};
use serde_json::{json, Value};
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

fn child_ids(body: &[u8]) -> Vec<String> {
    let v: Value = serde_json::from_slice(body).unwrap();
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            let s = i["id"]
                .as_str()
                .or_else(|| i["node_ref"].as_str())
                .unwrap_or("");
            s.strip_prefix("dev:").unwrap_or(s).to_string()
        })
        .collect()
}

fn child(id: &str, project: &str, parent: &str) -> NodeInput {
    let mut n = node(id, project);
    n.parent_id = Some(parent.into());
    n
}

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

fn setup() -> (tempfile::TempDir, MapStore) {
    let d = tempfile::tempdir().unwrap();
    let s = MapStore::new(d.path().join("map.sqlite"));
    s.migrate().unwrap();
    for p in ["p", "q"] {
        s.project(p, p, "review").unwrap();
        s.adopt_architecture(p, "v1", "design", "1234567890123456")
            .unwrap();
        s.architecture_node(
            &format!("{p}-arch"),
            p,
            None,
            "design",
            "component",
            "d",
            "1",
        )
        .unwrap();
        s.node(&node(&format!("{p}-node"), p)).unwrap();
        s.node(&node(&format!("{p}-node2"), p)).unwrap();
    }
    // Same architecture id in two projects/revisions: authorization must bind
    // the requested arch_version, not pick an arbitrary row.
    s.adopt_architecture("q", "v2", "design", "1234567890123456")
        .unwrap();
    s.architecture_node("shared", "p", None, "shared", "component", "d", "1")
        .unwrap();
    // Colon-containing/reserved-prefix ids in p, a q-only marker under a
    // `dev:x` id, and a p id `x` whose authorization must not be reused for
    // q's `dev:x`.
    s.node(&node("part:one", "p")).unwrap();
    s.node(&node("project:q", "p")).unwrap();
    s.node(&child("p-project-q-child", "p", "project:q"))
        .unwrap();
    s.node(&node("x", "p")).unwrap();
    s.node(&child("p-x-child", "p", "x")).unwrap();
    s.node(&node("y", "p")).unwrap();
    s.node(&node("dev:y", "p")).unwrap();
    s.node(&node("模块-一", "p")).unwrap();
    s.node(&node("dev:x", "q")).unwrap();
    s.node(&child("q-secret", "q", "dev:x")).unwrap();
    for n in ["p-node", "q-node"] {
        let sub: development_map::submission::Submission = serde_json::from_value(json!({
            "submission_key": format!("cp-{n}"),
            "checkpoint": {"node_id": n, "owner": "worker", "generation": 1, "version": 1,
                "digest": "0123456789abcdef0123456789abcdef", "summary": "review",
                "source_ref": "review.md"},
            "artifacts": []
        }))
        .unwrap();
        s.submit_bound(&sub, None).unwrap();
    }
    s.rebuild_projections().unwrap();
    (d, s)
}

const TP_R: &str = "matrix-reader-p-0123456789abcd";
const TQ_R: &str = "matrix-reader-q-0123456789abcd";
const TP_W: &str = "matrix-worker-p-0123456789abcd";
const TQ_W: &str = "matrix-worker-q-0123456789abcd";
const TP_M: &str = "matrix-manager-p-0123456789abcd";
const TQ_M: &str = "matrix-manager-q-0123456789abcd";

fn identities(s: &MapStore) {
    bind(s, TP_R, "p", "reader", None, 4102444800);
    bind(s, TQ_R, "q", "reader", None, 4102444800);
    bind(s, TP_W, "p", "worker", Some("p-node"), 4102444800);
    bind(s, TQ_W, "q", "worker", Some("q-node"), 4102444800);
    bind(s, TP_M, "p", "manager", None, 4102444800);
    bind(s, TQ_M, "q", "manager", None, 4102444800);
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

async fn post_json(
    app: &axum::Router,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, usize) {
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    if uri == "/v1/map/checkpoints" && body["version"] == 2 {
        let base = if body["node_id"] == "q-node" { 2 } else { 1 };
        b = b.header("x-map-base-checkpoint", base);
    }
    let r = app
        .clone()
        .oneshot(
            b.body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let st = r.status();
    let bytes = axum::body::to_bytes(r.into_body(), usize::MAX)
        .await
        .unwrap();
    (st, bytes.len())
}

/// (route template, allowed statuses for an authorized own-project read)
/// (route template, allowed statuses for an authorized own-project read,
///  expected status for another project's resource)
const GET_ROUTES: &[(&str, &[u16], u16)] = &[
    ("/v1/map/architecture?project_id={p}", &[200], 403),
    ("/v1/map/architecture/children?project_id={p}", &[200], 403),
    ("/v1/map/architecture/{p}-arch/traces", &[200], 404),
    ("/v1/map/architecture/{p}-arch/interfaces", &[200], 404),
    ("/v1/map/architecture/{p}-arch/neighborhood", &[200], 404),
    (
        "/v1/map/children?project_id={p}&parent_ref=project:{p}",
        &[200],
        404,
    ),
    (
        "/v1/map/navigation?project_id={p}&node_ref=dev:{p}-node",
        &[200],
        404,
    ),
    (
        "/v1/map/summary?project_id={p}&node_ref=dev:{p}-node",
        &[200],
        404,
    ),
    (
        "/v1/map/ancestors?project_id={p}&node_ref=dev:{p}-node",
        &[200],
        404,
    ),
    ("/v1/map/nodes/{p}-node", &[200], 404),
    ("/v1/map/nodes/{p}-node/checkpoints", &[200], 404),
    ("/v1/map/nodes/{p}-node/resources", &[200], 404),
    ("/v1/map/nodes/{p}-node/directory?key=x", &[404], 404),
    ("/v1/map/nodes/{p}-node/artifacts", &[200], 404),
    ("/v1/map/nodes/{p}-node/dependencies", &[200], 404),
    ("/v1/map/nodes/{p}-node/architecture-traces", &[200], 404),
    (
        "/v1/map/nodes/{p}-node/checkpoint-artifacts?checkpoint_id={cp}",
        &[200],
        404,
    ),
    ("/v1/map/nodes/{p}-node/local-files", &[404], 404),
    (
        "/v1/map/artifacts/lookup?repo_key=x&revision=y&path=z",
        &[403],
        403,
    ),
    (
        "/v1/map/resources/lookup?project_id={p}&repo_key=r&revision=v&path=p",
        &[400],
        403,
    ),
    ("/v1/map/local-source/status?project_id={p}", &[200], 403),
    (
        "/v1/map/local-source/file?project_id={p}&path=x",
        &[404],
        403,
    ),
    ("/v1/map/local-source/tree?project_id={p}", &[404], 403),
    (
        "/v1/map/local-source/lookup?project_id={p}&path=x",
        &[404],
        403,
    ),
    ("/v1/map/local-source/history?node_id={p}-node", &[200], 404),
    ("/v1/map/changes?project_id={p}", &[200], 403),
    ("/v1/map/context?node_id={p}-node", &[200], 404),
    ("/v1/map/search?project_id={p}&q=ab", &[200], 403),
];

#[tokio::test]
async fn get_route_matrix_is_project_and_role_scoped() {
    let (_d, s) = setup();
    identities(&s);
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    for (template, allowed, foreign_status) in GET_ROUTES {
        let own = template.replace("{p}", "p").replace("{cp}", "1");
        let foreign = template.replace("{p}", "q").replace("{cp}", "2");
        let (st, _len, _b) = call(&app, "GET", &own, None).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "no token {own}");
        let (st, _len, _b) = call(&app, "GET", &own, Some(TP_R)).await;
        assert!(
            allowed.contains(&(st.as_u16())),
            "own read {own}: {st} not in {allowed:?}"
        );
        let (st, len, _b) = call(&app, "GET", &foreign, Some(TP_R)).await;
        assert_eq!(st.as_u16(), *foreign_status, "foreign read {foreign}");
        assert_eq!(len, 0, "foreign read {foreign} must not return a body");
        let (st, _len, _b) = call(&app, "GET", &foreign, Some(TQ_R)).await;
        assert!(
            allowed.contains(&(st.as_u16())) || st == StatusCode::NOT_FOUND,
            "own read for q {foreign}: {st}"
        );
        if allowed.contains(&200) {
            let (hst, hlen, _b) = call(&app, "HEAD", &own, Some(TP_R)).await;
            assert_eq!(hst, StatusCode::OK, "HEAD {own}");
            assert_eq!(hlen, 0, "HEAD must not return a body");
            let (hst, hlen, _b) = call(&app, "HEAD", &foreign, Some(TP_R)).await;
            assert_eq!(hst.as_u16(), *foreign_status, "HEAD foreign {foreign}");
            assert_eq!(hlen, 0);
        }
    }
    // `parent_id` is a raw dev-node id and must work with and without the
    // project_id navigation shortcut, including ids that contain colons.
    for uri in [
        "/v1/map/children?parent_id=p-node",
        "/v1/map/children?parent_id=p-node&project_id=p",
        "/v1/map/children?parent_id=part:one",
        "/v1/map/children?parent_id=part:one&project_id=p",
    ] {
        let (st, _len, _b) = call(&app, "GET", uri, Some(TP_R)).await;
        assert_eq!(st, StatusCode::OK, "{uri}");
    }
    for (uri, want) in [
        (
            "/v1/map/children?parent_id=p-node&project_id=p",
            "dev:p-node",
        ),
        (
            "/v1/map/children?parent_id=part:one&project_id=p",
            "dev:part:one",
        ),
    ] {
        let (st, _len, body) = call(&app, "GET", uri, Some(TP_R)).await;
        assert_eq!(st, StatusCode::OK, "{uri}");
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["parent_ref"], want, "{uri}");
    }
}

#[tokio::test]
async fn children_raw_ids_cannot_cross_projects() {
    let (_d, s) = setup();
    identities(&s);
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    // p's node `x` must never authorize q's node `dev:x`; raw parent_id is
    // used for both authorization and the exact-node store lookup.
    for (uri, want) in [
        (
            "/v1/map/children?parent_id=dev:project:q",
            StatusCode::NOT_FOUND,
        ),
        ("/v1/map/children?parent_id=project:q", StatusCode::OK),
        (
            "/v1/map/children?parent_id=dev:project:q&project_id=p",
            StatusCode::NOT_FOUND,
        ),
        (
            "/v1/map/children?parent_id=project:q&project_id=p",
            StatusCode::OK,
        ),
        ("/v1/map/children?parent_id=dev:x", StatusCode::NOT_FOUND),
        (
            "/v1/map/children?parent_id=dev:x&project_id=p",
            StatusCode::NOT_FOUND,
        ),
    ] {
        let (st, _len, body) = call(&app, "GET", uri, Some(TP_R)).await;
        assert_eq!(st, want, "{uri}: {st}");
        let text = String::from_utf8_lossy(&body);
        assert!(!text.contains("q-secret"), "{uri} leaked q data: {text}");
    }
    // p's own raw id `x` returns exactly p's child, not q's `dev:x` child.
    for uri in [
        "/v1/map/children?parent_id=x",
        "/v1/map/children?parent_id=x&project_id=p",
    ] {
        let (st, _len, body) = call(&app, "GET", uri, Some(TP_R)).await;
        assert_eq!(st, StatusCode::OK, "{uri}");
        assert_eq!(child_ids(&body), vec!["p-x-child"], "{uri}");
    }
    // `project:q` is a raw p node here: it must return p's child, never q's roots.
    for uri in [
        "/v1/map/children?parent_id=project:q",
        "/v1/map/children?parent_id=project:q&project_id=p",
    ] {
        let (st, _len, body) = call(&app, "GET", uri, Some(TP_R)).await;
        assert_eq!(st, StatusCode::OK, "{uri}");
        assert_eq!(child_ids(&body), vec!["p-project-q-child"], "{uri}");
        let text = String::from_utf8_lossy(&body);
        assert!(
            !text.contains("q-node") && !text.contains("dev:x") && !text.contains("q-secret"),
            "{uri} leaked q data: {text}"
        );
    }
    // q can still read its own raw `dev:x` children, with and without project_id.
    for uri in [
        "/v1/map/children?parent_id=dev:x",
        "/v1/map/children?parent_id=dev:x&project_id=q",
    ] {
        let (st, _len, body) = call(&app, "GET", uri, Some(TQ_R)).await;
        assert_eq!(st, StatusCode::OK, "{uri}");
        assert_eq!(child_ids(&body), vec!["q-secret"], "{uri}");
    }
}

#[tokio::test]
async fn architecture_auth_binds_revision() {
    let (_d, s) = setup();
    // Simulate a legacy/imported database where the same architecture id was
    // written under a second project/version, bypassing the API's uniqueness
    // guard.  The authorization lookup must still not pick an arbitrary row.
    let conn = rusqlite::Connection::open(_d.path().join("map.sqlite")).unwrap();
    conn.execute(
        "INSERT INTO map_arch_nodes(id,project_id,arch_version,parent_id,title,kind,source_ref,sort_key) VALUES('shared','q','v2',NULL,'shared','component','d','1')",
        [],
    )
    .unwrap();
    identities(&s);
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    // Same id `shared` exists in p/v1 and q/v2. Without a revision it is
    // ambiguous and must not resolve to either project.
    for base in ["traces", "interfaces", "neighborhood"] {
        let uri = format!("/v1/map/architecture/shared/{base}");
        let (st, _len, _b) = call(&app, "GET", &uri, Some(TP_R)).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{uri}: {st}");
    }
    // A revision selects exactly one owner; the other project's token is denied.
    for base in ["traces", "interfaces", "neighborhood"] {
        let p_ok = format!("/v1/map/architecture/shared/{base}?revision=v1");
        let (st, _len, _b) = call(&app, "GET", &p_ok, Some(TP_R)).await;
        assert_eq!(st, StatusCode::OK, "{p_ok}");
        let p_foreign = format!("/v1/map/architecture/shared/{base}?revision=v2");
        let (st, _len, _b) = call(&app, "GET", &p_foreign, Some(TP_R)).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{p_foreign}");
        let q_ok = format!("/v1/map/architecture/shared/{base}?revision=v2");
        let (st, _len, _b) = call(&app, "GET", &q_ok, Some(TQ_R)).await;
        assert_eq!(st, StatusCode::OK, "{q_ok}");
    }
    // project_id cannot override the revision's owner.
    for uri in [
        "/v1/map/architecture/shared/traces?revision=v2&project_id=p",
        "/v1/map/architecture/shared/traces?revision=v1&project_id=q",
    ] {
        let (st, _len, _b) = call(&app, "GET", uri, Some(TP_R)).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn resume_keeps_raw_node_id_and_typed_node_ref_separate() {
    let (_d, s) = setup();
    identities(&s);
    let raw = s
        .agent_resume(
            TP_R,
            &Resume {
                project_id: "p".into(),
                node_id: Some("dev:y".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(raw["node"]["id"], "dev:y");
    let typed = s
        .agent_resume(
            TP_R,
            &Resume {
                project_id: "p".into(),
                node_ref: Some("dev:dev:y".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(typed["node"]["id"], "dev:y");
    let plain = s
        .agent_resume(
            TP_R,
            &Resume {
                project_id: "p".into(),
                node_id: Some("y".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(plain["node"]["id"], "y");
}

#[tokio::test]
async fn unrelated_tasks_table_does_not_change_worker_auth() {
    let (_d, s) = setup();
    identities(&s);
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    let conn = rusqlite::Connection::open(_d.path().join("map.sqlite")).unwrap();
    conn.execute_batch("CREATE TABLE tasks (unrelated TEXT)")
        .unwrap();
    let (st, _len, _b) = call(&app, "GET", "/v1/map/projects", Some(TP_W)).await;
    assert_eq!(st, StatusCode::OK, "unrelated tasks table broke auth: {st}");
    // Explicit opt-in plus a mismatched host task must still be rejected.
    conn.execute_batch(
        "DROP TABLE tasks; CREATE TABLE tasks (id TEXT, node_id TEXT);          INSERT INTO tasks VALUES ('worker-p','other-node');          CREATE TABLE map_host_compat (enabled INTEGER)",
    )
    .unwrap();
    let (st, _len, _b) = call(&app, "GET", "/v1/map/projects", Some(TP_W)).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "mismatched host task: {st}");
}

#[tokio::test]
async fn encoded_and_unicode_node_ids_resolve_without_prefix_guessing() {
    let (_d, s) = setup();
    identities(&s);
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    for uri in [
        "/v1/map/children?parent_id=part%3Aone",
        "/v1/map/children?parent_id=part%3Aone&project_id=p",
        "/v1/map/children?parent_id=%E6%A8%A1%E5%9D%97-%E4%B8%80",
        "/v1/map/children?parent_id=%E6%A8%A1%E5%9D%97-%E4%B8%80&project_id=p",
    ] {
        let (st, _len, body) = call(&app, "GET", uri, Some(TP_R)).await;
        assert_eq!(st, StatusCode::OK, "{uri}: {st}");
        assert!(
            !String::from_utf8_lossy(&body).contains("q-secret"),
            "{uri}"
        );
    }
}

#[test]
fn idempotent_arch_writes_do_not_emit_events() {
    let (_d, s) = setup();
    s.architecture_node("p-arch2", "p", None, "arch2", "component", "d", "2")
        .unwrap();
    let count = |kind: &str| -> i64 {
        s.read(|tx| {
            Ok(tx.query_row(
                "SELECT count(*) FROM map_events WHERE project_id='p' AND kind=?",
                [kind],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    };
    let before = count("architecture_edge");
    s.architecture_edge("p-arch", "p-arch2", "interface", "review")
        .unwrap();
    s.architecture_edge("p-arch", "p-arch2", "interface", "review")
        .unwrap();
    assert_eq!(count("architecture_edge"), before + 1);
    let before = count("architecture_trace");
    s.trace("p-arch", "p-node", "supports", "review").unwrap();
    s.trace("p-arch", "p-node", "supports", "review").unwrap();
    assert_eq!(count("architecture_trace"), before + 1);
    let before = count("node");
    s.node(&node("p-node", "p")).unwrap();
    assert_eq!(count("node"), before);
}

#[tokio::test]
async fn worker_scope_is_own_node_subtree() {
    let (_d, s) = setup();
    identities(&s);
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    let (st, _len, _b) = call(&app, "GET", "/v1/map/children?parent_id=p-node", Some(TP_W)).await;
    assert_eq!(st, StatusCode::OK, "worker own node");
    let (st, _len, _b) = call(
        &app,
        "GET",
        "/v1/map/children?parent_id=p-node2",
        Some(TP_W),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "worker sibling node");
}

#[tokio::test]
async fn checkpoint_history_is_readable_by_a_new_session_token() {
    let (_d, s) = setup();
    identities(&s);
    let new_token = "matrix-reader-p-session2-0123456789";
    bind(&s, new_token, "p", "reader", None, 4102444800);
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    let (st, _len, body) = call(
        &app,
        "GET",
        "/v1/map/nodes/p-node/checkpoints",
        Some(new_token),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["items"][0]["submission_key"], "cp-p-node");
    let (st, _len, _b) = call(&app, "GET", "/v1/map/nodes/p-node/checkpoints", Some(TQ_R)).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn manager_assigns_worker_but_worker_cannot_assign() {
    let (_d, s) = setup();
    identities(&s);
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    let mut assigned = node("assigned-node", "p");
    assigned.owner = Some("worker".into());
    assigned.generation = 1;
    let (st, _len) = post_json(
        &app,
        "/v1/map/nodes",
        Some(TP_W),
        serde_json::to_value(&assigned).unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "worker assignment");
    let (st, _len) = post_json(
        &app,
        "/v1/map/nodes",
        Some(TP_M),
        serde_json::to_value(&assigned).unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "manager assignment");
    let (st, _len, body) = call(&app, "GET", "/v1/map/nodes/assigned-node", Some(TP_R)).await;
    assert_eq!(st, StatusCode::OK);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["owner"], "worker");
    assert_eq!(v["generation"], 1);
}

#[tokio::test]
async fn projects_are_listed_only_for_the_bound_project() {
    let (_d, s) = setup();
    identities(&s);
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    let (st, _len, body) = call(&app, "GET", "/v1/map/projects", Some(TP_R)).await;
    assert_eq!(st, StatusCode::OK);
    let v: Value = serde_json::from_slice(&body).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], "p");
    assert!(items[0]["node_count"].as_i64().unwrap() > 0, "{v}");
    let (st, _len, body) = call(&app, "GET", "/v1/map/projects", Some(TQ_R)).await;
    assert_eq!(st, StatusCode::OK);
    let v: Value = serde_json::from_slice(&body).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], "q");
}

#[tokio::test]
async fn resource_reads_hide_missing_and_cross_project_existence() {
    let (_d, s) = setup();
    identities(&s);
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    // Missing, foreign and project-mismatch all collapse to 404 for path/ref
    // selected resources, so a reader cannot use 403 vs 404 as an existence
    // oracle.
    for uri in [
        "/v1/map/nodes/missing?project_id=p",
        "/v1/map/nodes/missing",
        "/v1/map/nodes/q-node?project_id=p",
        "/v1/map/nodes/q-node",
        "/v1/map/nodes/p-node?project_id=q",
        "/v1/map/nodes/missing?project_id=q",
        "/v1/map/architecture/missing/traces?project_id=p",
        "/v1/map/architecture/missing/traces?project_id=q",
        "/v1/map/architecture/q-arch/traces?project_id=p",
    ] {
        let (st, len, _b) = call(&app, "GET", uri, Some(TP_R)).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{uri}: {st}");
        assert_eq!(len, 0, "{uri} must not return a body");
    }
    // A worker cannot probe other node ids, even when they do not exist.
    for uri in [
        "/v1/map/nodes/missing?project_id=p",
        "/v1/map/nodes/q-node?project_id=p",
    ] {
        let (st, _len, _b) = call(&app, "GET", uri, Some(TP_W)).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{uri}: {st}");
    }
    // Managers see the same 404 for out-of-scope resources.
    let (st, _len, _b) = call(&app, "GET", "/v1/map/nodes/q-node", Some(TP_M)).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn local_source_without_snapshot_is_404_and_history_uses_node_project() {
    let (_d, s) = setup();
    identities(&s);
    let snapshot = json!({
        "project_id": "p", "name": "p", "core_node": "p-node",
        "source_root": "/tmp", "config_digest": "d", "digest": "d",
        "observed_at": 1, "stale_after_seconds": 60,
        "nodes": [], "files": [], "links": []
    })
    .to_string();
    s.connect(false)
        .unwrap()
        .execute(
            "INSERT INTO map_local_history(project_id,version,digest,observed_at,snapshot) VALUES('p',1,'d',unixepoch(),?)",
            [snapshot],
        )
        .unwrap();
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    for uri in [
        "/v1/map/local-source/tree?project_id=p",
        "/v1/map/local-source/lookup?project_id=p&path=x",
        "/v1/map/nodes/p-node/local-files",
    ] {
        let (st, _len, _b) = call(&app, "GET", uri, Some(TP_R)).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{uri}: {st}");
    }
    // node_id alone must resolve the authorized project instead of reading "".
    for uri in [
        "/v1/map/local-source/history?node_id=p-node",
        "/v1/map/local-source/history?node_id=p-node&project_id=p",
    ] {
        let (st, _len, body) = call(&app, "GET", uri, Some(TP_R)).await;
        assert_eq!(st, StatusCode::OK, "{uri}");
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 1, "{uri}");
    }
    let (st, _len, _b) = call(
        &app,
        "GET",
        "/v1/map/local-source/history?node_id=q-node",
        Some(TP_R),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_route_matrix_is_role_and_project_scoped() {
    let (_d, s) = setup();
    identities(&s);
    let expired = "matrix-expired-reader-0123456789";
    let revoked = "matrix-revoked-reader-0123456789";
    bind(&s, expired, "p", "reader", None, 1);
    bind(&s, revoked, "p", "reader", None, 4102444800);
    s.revoke_identity(revoked).unwrap();
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    let cp = |node: &str, v: i64| {
        json!({"node_id":node,"owner":"worker","generation":1,"version":v,
            "digest":"0123456789abcdef0123456789abcdef","summary":"review",
            "source_ref":"review.md"})
    };

    // Unauthenticated, expired and revoked identities are 401, not 403.
    for token in [None, Some(expired), Some(revoked)] {
        let (st, _len) = post_json(&app, "/v1/map/checkpoints", token, cp("p-node", 2)).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "token {token:?}");
    }
    // A valid reader cannot write a worker checkpoint.
    let (st, _len) = post_json(&app, "/v1/map/checkpoints", Some(TP_R), cp("p-node", 2)).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // A manager is not automatically a worker submission identity.
    let (st, _len) = post_json(&app, "/v1/map/checkpoints", Some(TP_M), cp("p-node", 2)).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // Worker own node succeeds; another project's node is denied.
    let (st, _len) = post_json(&app, "/v1/map/checkpoints", Some(TP_W), cp("p-node", 2)).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _len) = post_json(&app, "/v1/map/checkpoints", Some(TP_W), cp("q-node", 2)).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _len) = post_json(&app, "/v1/map/checkpoints", Some(TQ_W), cp("q-node", 2)).await;
    assert_eq!(st, StatusCode::OK);

    // Submission idempotency through the bound HTTP path.
    let bundle = |summary: &str| {
        json!({"base_checkpoint_rowid":3,"submission_key":"matrix-submit-key",
            "checkpoint":{"node_id":"p-node","owner":"worker","generation":1,"version":3,
                "digest":"0123456789abcdef0123456789abcdef","summary":summary,
                "source_ref":"review.md"},
            "artifacts":[]})
    };
    let (st, _len) = post_json(&app, "/v1/map/submissions", Some(TP_W), bundle("one")).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _len) = post_json(&app, "/v1/map/submissions", Some(TP_W), bundle("one")).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _len) = post_json(&app, "/v1/map/submissions", Some(TP_W), bundle("two")).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (st, _len) = post_json(&app, "/v1/map/submissions", Some(TQ_W), bundle("one")).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Rejected acceptance is authorized for the project manager only.
    let accept = json!({"node_id":"p-node","generation":1,"version":3,
        "digest":"0123456789abcdef0123456789abcdef","decision":"rejected",
        "evidence_level":"document","source_ref":"review"});
    let (st, _len) = post_json(&app, "/v1/map/acceptances", Some(TP_M), accept.clone()).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _len) = post_json(&app, "/v1/map/acceptances", Some(TQ_M), accept.clone()).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Direct artifact writes remain operator-only; scoped bundles carry artifacts.
    let (st, _len) = post_json(
        &app,
        "/v1/map/artifacts",
        Some(TP_M),
        json!({"node_id":"p-node","repo_key":"r","revision":"v","path":"p",
            "kind":"file","provenance":"declared","verification":"unverified"}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Node/architecture management is project-scoped manager authority.
    let node_body = |id: &str, project: &str| {
        json!({"id":id,"project_id":project,"parent_id":null,"kind":"module",
            "title":id,"goal":"g","sort_key":"s","plan_state":"active",
            "owner":"worker","generation":1,"architecture_ref":null})
    };
    let (st, _len) = post_json(&app, "/v1/map/nodes", Some(TP_M), node_body("p-new", "p")).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _len) = post_json(&app, "/v1/map/nodes", Some(TP_M), node_body("q-new", "q")).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _len) = post_json(&app, "/v1/map/nodes", Some(TP_M), node_body("p-node", "q")).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _len) = post_json(&app, "/v1/map/nodes", Some(TP_M), node_body("q-node", "p")).await;
    assert_eq!(st, StatusCode::CONFLICT);
    // Handoff of one's own node succeeds; another project's node is denied.
    let handoff = json!({"node_id":"p-node2","expected_owner":"worker",
        "expected_generation":1,"new_owner":"next","source_ref":"review"});
    let (st, _len) = post_json(&app, "/v1/map/handoffs", Some(TP_M), handoff).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _len) = post_json(
        &app,
        "/v1/map/handoffs",
        Some(TP_M),
        json!({"node_id":"q-node","expected_owner":"worker",
            "expected_generation":1,"new_owner":"next","source_ref":"review"}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Trace before adopting a newer architecture version.
    let (st, _len) = post_json(
        &app,
        "/v1/map/architecture/traces",
        Some(TP_M),
        json!({"arch_node_id":"p-arch","dev_node_id":"p-node2",
            "relation":"tests","source_ref":"review"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _len) = post_json(
        &app,
        "/v1/map/architecture/traces",
        Some(TP_M),
        json!({"arch_node_id":"q-arch","dev_node_id":"q-node",
            "relation":"tests","source_ref":"review"}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    let adopt = json!({"project_id":"p","version":"v2","source_ref":"s",
        "digest":"1234567890123456","variant":"v","adoption_ref":"a"});
    let (st, _len) = post_json(
        &app,
        "/v1/map/architecture/adopt",
        Some(TP_M),
        adopt.clone(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _len) = post_json(
        &app,
        "/v1/map/architecture/adopt",
        Some(TP_M),
        json!({"project_id":"q","version":"v2","source_ref":"s",
            "digest":"1234567890123456","variant":"v","adoption_ref":"a"}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn non_scoped_mode_separates_read_and_write_tokens() {
    let (_d, s) = setup();
    let read = "non-scoped-read-bearer-0123456789";
    let worker = "non-scoped-worker-token-0123456789";
    let manager = "non-scoped-manager-token-0123456789";
    let app = MapWeb::new(s.clone(), read.into())
        .with_write_tokens(worker.into(), manager.into())
        .unwrap()
        .router();
    let (st, _len, _b) = call(&app, "GET", "/v1/map/projects", None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _len, _b) = call(&app, "GET", "/v1/map/projects", Some(read)).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _len, _b) = call(&app, "GET", "/v1/map/projects", Some(worker)).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let cp = json!({"node_id":"p-node","owner":"worker","generation":1,"version":5,
        "digest":"0123456789abcdef0123456789abcdef","summary":"review",
        "source_ref":"review.md"});
    let (st, _len) = post_json(&app, "/v1/map/checkpoints", Some(worker), cp.clone()).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _len) = post_json(&app, "/v1/map/checkpoints", Some(manager), cp).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _len) = post_json(
        &app,
        "/v1/map/nodes",
        Some(manager),
        json!({"id":"p-new","project_id":"p","parent_id":null,"kind":"module",
            "title":"new","goal":"g","sort_key":"s","plan_state":"active",
            "owner":"worker","generation":1,"architecture_ref":null}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _len) = post_json(
        &app,
        "/v1/map/nodes",
        Some(read),
        json!({"id":"p-new2","project_id":"p","parent_id":null,"kind":"module",
            "title":"new","goal":"g","sort_key":"s","plan_state":"active",
            "owner":"worker","generation":1,"architecture_ref":null}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

#[test]
fn protocol_document_matches_schema_version() {
    let doc = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/PROTOCOL.md"),
    )
    .unwrap();
    let declared = doc
        .split("current map schema version is ")
        .nth(1)
        .unwrap()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap();
    assert_eq!(
        declared,
        development_map::recovery::SCHEMA_VERSION.to_string(),
        "docs/PROTOCOL.md and recovery::SCHEMA_VERSION disagree"
    );
}

#[tokio::test]
async fn closeout_artifact_scope() {
    let (d, s) = setup();
    identities(&s);
    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .arg(d.path())
        .status()
        .unwrap()
        .success());
    s.register_repository("r", d.path(), "fixture").unwrap();
    s.register_project_repository("q", "r").unwrap();
    let app = MapWeb::new(s.clone(), String::new())
        .with_scoped_identities()
        .router();
    let bundle = |p: &str| {
        let n = format!("{p}-node");
        json!({"base_checkpoint_rowid":if p=="q"{2}else{1},"submission_key":"scope","checkpoint":{"node_id":n,"owner":"worker","generation":1,"version":2,"digest":"1234567890123456","summary":"s","source_ref":"s"},"artifacts":[{"node_id":n,"repo_key":"r","revision":"abc","path":"private.rs","kind":"file","verification":"unverified","provenance":"declared","content_digest":if p=="q"{None}else{Some("attacker")}}]})
    };
    assert_eq!(
        post_json(&app, "/v1/map/submissions", Some(TQ_W), bundle("q"))
            .await
            .0,
        StatusCode::OK
    );
    let db = s.connect(false).unwrap();
    db.execute("UPDATE map_artifacts SET verification='verified'", [])
        .unwrap();
    let counts = || {
        db.query_row("SELECT (SELECT count(*) FROM map_events),(SELECT count(*) FROM map_checkpoint_index),(SELECT count(*) FROM map_node_artifacts)",[],|r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?))).unwrap()
    };
    let before = counts();
    assert!(
        post_json(&app, "/v1/map/submissions", Some(TP_W), bundle("p"))
            .await
            .0
            .is_client_error()
    );
    assert_eq!(before, counts());
    s.register_project_repository("p", "r").unwrap();
    assert_eq!(
        post_json(&app, "/v1/map/submissions", Some(TP_W), bundle("p"))
            .await
            .0,
        StatusCode::OK
    );
    for p in ["p", "q"] {
        let v = s
            .read(|tx| development_map::store::artifacts(tx, &format!("{p}-node"), 0, 10, None))
            .unwrap();
        let a = &v["items"][0];
        assert_eq!(
            a["verification"],
            if p == "q" { "verified" } else { "unverified" }
        );
        if p == "q" {
            assert!(a["content_digest"].is_null());
        }
    }
}
#[tokio::test]
async fn closeout_dirty_relations() {
    let (_d, s) = setup();
    identities(&s);
    s.connect(false).unwrap().execute_batch("INSERT INTO map_arch_traces VALUES('p-arch','v1','q-node','implements','SECRET'); INSERT INTO map_architecture_edges VALUES('p-arch','q-arch','v1','uses','SECRET'); INSERT INTO map_arch_traces VALUES('p-arch','v1','p-node','implements','LOCAL'); INSERT INTO map_arch_traces VALUES('q-arch','v1','p-node','implements','SECRET');").unwrap();
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    for token in [TP_R, TP_W] {
        for path in [
            "architecture/p-arch/traces",
            "architecture/p-arch/interfaces",
            "architecture/p-arch/neighborhood",
            "nodes/p-node/architecture-traces",
        ] {
            let (status, _, body) =
                call(&app, "GET", &format!("/v1/map/{path}"), Some(token)).await;
            if token == TP_W && path.starts_with("architecture/") {
                assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
                assert!(body.is_empty());
                continue;
            }
            assert_eq!(status, StatusCode::OK, "{path}");
            let text = String::from_utf8(body).unwrap();
            assert!(
                !text.contains("SECRET") && !text.contains("q-arch") && !text.contains("q-node"),
                "{path}: {text}"
            );
            if path.ends_with("traces") {
                assert!(text.contains("LOCAL"));
            }
        }
    }
}

#[tokio::test]
async fn migrated_v8_shared_artifact_is_quarantined() {
    let (_d, s) = setup();
    let db = s.connect(false).unwrap();
    // Reconstruct the v8 global table and both kinds of historical links.
    db.execute_batch("PRAGMA foreign_keys=OFF; DROP VIEW map_local_node_artifacts; DROP VIEW map_local_checkpoint_artifacts; DROP TABLE map_artifacts;
 CREATE TABLE map_artifacts(id INTEGER PRIMARY KEY,repo_key TEXT NOT NULL,revision TEXT NOT NULL,path TEXT NOT NULL,kind TEXT NOT NULL,verification TEXT NOT NULL,content_digest TEXT,UNIQUE(repo_key,revision,path,kind));
 INSERT INTO map_repositories VALUES('r','/fixture','fixture');INSERT INTO map_project_repositories VALUES('q','r');
 INSERT INTO map_artifacts VALUES(12,'r','abc','SECRET.rs','file','verified','polluted');
 INSERT INTO map_node_artifacts VALUES('p-node',12,'declared'),('q-node',12,'declared');
 INSERT INTO map_checkpoint_artifacts SELECT id,12,'declared' FROM map_checkpoint_index;
 UPDATE map_schema_migrations SET version=8 WHERE version=(SELECT MAX(version) FROM map_schema_migrations);").unwrap();
    drop(db);
    s.migrate().unwrap();
    identities(&s);
    let db = s.connect(true).unwrap();
    assert!(db.query_row("SELECT a.project_id IS NULL AND a.verification='unverified' AND a.content_digest IS NULL AND h.content_digest='polluted' FROM map_artifacts a JOIN map_legacy_artifact_verifications h ON h.artifact_id=a.id WHERE a.id=12",[],|r|r.get::<_,bool>(0)).unwrap());
    development_map::recovery::integrity(&db).unwrap();
    let app = MapWeb::new(s, String::new())
        .with_scoped_identities()
        .router();
    for (p, t, id) in [("p", TP_R, 1), ("p", TP_W, 1), ("q", TQ_R, 2)] {
        for uri in [
            format!("/v1/map/nodes/{p}-node/artifacts"),
            format!("/v1/map/nodes/{p}-node/checkpoint-artifacts?checkpoint_id={id}"),
        ] {
            let (st, _, body) = call(&app, "GET", &uri, Some(t)).await;
            assert_eq!(st, StatusCode::OK);
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap()["items"],
                json!([])
            );
        }
    }
}

#[tokio::test]
async fn worker_cannot_supply_digest_for_verified_peer_artifact() {
    let (dir, store) = setup();
    identities(&store);
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q"]);
    std::fs::write(dir.path().join("fact"), b"fact").unwrap();
    git(&["add", "fact"]);
    git(&[
        "-c",
        "user.name=QA",
        "-c",
        "user.email=qa@example.invalid",
        "commit",
        "-qm",
        "fixture",
    ]);
    let revision = git(&["rev-parse", "HEAD"]);
    store
        .register_repository("r", dir.path(), "fixture")
        .unwrap();
    store.register_project_repository("p", "r").unwrap();
    let mut artifact = development_map::store::ArtifactInput {
        node_id: "p-node2".into(),
        repo_key: "r".into(),
        revision: revision.clone(),
        path: "fact".into(),
        kind: "file".into(),
        provenance: "observed".into(),
        verification: "verified".into(),
        content_digest: None,
    };
    store.artifact_checked(&artifact, dir.path()).unwrap();
    let app = MapWeb::new(store.clone(), String::new())
        .with_scoped_identities()
        .router();
    let bundle = |digest: Option<&str>| json!({"base_checkpoint_rowid":1,"submission_key":"trust","checkpoint":{"node_id":"p-node","generation":1,"version":2,"owner":"worker","digest":"1234567890123456","summary":"claim","source_ref":"fact"},"artifacts":[{"node_id":"p-node","repo_key":"r","revision":revision,"path":"fact","kind":"file","verification":"unverified","provenance":"declared","content_digest":digest}]});
    let post = |request| post_json(&app, "/v1/map/submissions", Some(TP_W), request);
    let state = || {
        store
            .read(|tx| development_map::store::artifacts(tx, "p-node2", 0, 5, None))
            .unwrap()["items"][0]
            .clone()
    };
    let before = state();
    assert_eq!(post(bundle(Some("fake"))).await.0, StatusCode::CONFLICT);
    assert_eq!(state(), before);
    artifact.content_digest = Some("fake".into());
    assert!(store.artifact_checked(&artifact, dir.path()).is_err());
    use sha2::{Digest, Sha256};
    let hash = format!("{:x}", Sha256::digest(b"fact"));
    artifact.content_digest = Some(hash.clone());
    store.artifact_checked(&artifact, dir.path()).unwrap();
    assert_eq!(
        post(bundle(Some("different"))).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(state()["content_digest"], hash);
    let counts = || {
        store.connect(true).unwrap().query_row("SELECT (SELECT count(*) FROM map_events),(SELECT count(*) FROM map_checkpoint_index),(SELECT count(*) FROM map_node_artifacts)",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?))).unwrap()
    };
    assert_eq!(post(bundle(None)).await.0, StatusCode::OK);
    let peer_links:i64=store.connect(true).unwrap().query_row(
        "SELECT COUNT(*) FROM map_local_node_artifacts l JOIN map_artifacts a ON a.id=l.artifact_id WHERE l.node_id='p-node' AND a.path='fact' AND a.project_id='p' AND a.content_digest=?",
        [&hash], |r|r.get(0)).unwrap();
    assert_eq!(peer_links, 1);
    let committed = counts();
    assert_eq!(post(bundle(None)).await.0, StatusCode::OK);
    assert_eq!(counts(), committed);
    // A declared digest must also disappear on a later existence-only promotion.
    artifact.path = "other".into();
    std::fs::write(dir.path().join("other"), b"other").unwrap();
    git(&["add", "other"]);
    git(&[
        "-c",
        "user.name=QA",
        "-c",
        "user.email=qa@example.invalid",
        "commit",
        "-qm",
        "other",
    ]);
    artifact.revision = git(&["rev-parse", "HEAD"]);
    artifact.verification = "unverified".into();
    artifact.content_digest = Some("declared".into());
    store.artifact(&artifact).unwrap();
    artifact.content_digest = None;
    store.artifact_checked(&artifact, dir.path()).unwrap();
    let db = store.connect(true).unwrap();
    assert!(db.query_row("SELECT verification='verified' AND content_digest IS NULL FROM map_artifacts WHERE path='other'",[],|r|r.get::<_,bool>(0)).unwrap());
}

#[tokio::test]
async fn v9_verified_claims_lose_authority_until_git_recheck() {
    let (dir, s) = setup();
    identities(&s);
    let git = |args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(["-C"])
            .arg(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success());
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q"]);
    std::fs::write(dir.path().join("fact"), b"original").unwrap();
    git(&["add", "fact"]);
    git(&[
        "-c",
        "user.name=QA",
        "-c",
        "user.email=qa@example.invalid",
        "commit",
        "-qm",
        "original",
    ]);
    let rev = git(&["rev-parse", "HEAD"]);
    s.register_repository("r", dir.path(), "fixture").unwrap();
    s.register_project_repository("p", "r").unwrap();
    let db = s.connect(false).unwrap();
    db.execute("INSERT INTO map_artifacts(id,project_id,repo_key,revision,path,kind,verification,content_digest) VALUES(88,'p','r',?,'fact','file','verified','FORGED')",[&rev]).unwrap();
    db.execute_batch("INSERT INTO map_node_artifacts VALUES('p-node',88,'observed');INSERT INTO map_checkpoint_artifacts VALUES(1,88,'observed'); UPDATE map_schema_migrations SET version=9 WHERE version=10;").unwrap();
    drop(db);
    assert!(s
        .read(|tx| development_map::store::artifacts(tx, "p-node", 0, 5, None))
        .is_err());
    s.migrate().unwrap();
    let app = MapWeb::new(s.clone(), String::new())
        .with_scoped_identities()
        .router();
    for token in [TP_R, TP_W] {
        for path in ["artifacts", "checkpoint-artifacts?checkpoint_id=1"] {
            let (status, _, body) = call(
                &app,
                "GET",
                &format!("/v1/map/nodes/p-node/{path}"),
                Some(token),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let v: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(v["items"][0]["verification"], "unverified");
            assert!(v["items"][0]["content_digest"].is_null());
        }
    }
    let db = s.connect(true).unwrap();
    assert!(db.query_row("SELECT content_digest='FORGED' FROM map_legacy_artifact_verifications WHERE artifact_id=88",[],|r|r.get::<_,bool>(0)).unwrap());
    use sha2::{Digest, Sha256};
    let digest = format!("{:x}", Sha256::digest(b"original"));
    let mut a = development_map::store::ArtifactInput {
        node_id: "p-node".into(),
        repo_key: "r".into(),
        revision: rev,
        path: "fact".into(),
        kind: "file".into(),
        verification: "verified".into(),
        provenance: "observed".into(),
        content_digest: Some("FORGED".into()),
    };
    assert!(s.artifact_checked(&a, dir.path()).is_err());
    a.content_digest = Some(digest.clone());
    s.artifact_checked(&a, dir.path()).unwrap();
    s.migrate().unwrap();
    let v = s
        .read(|tx| development_map::store::artifacts(tx, "p-node", 0, 5, None))
        .unwrap();
    assert_eq!(v["items"][0]["verification"], "verified");
    assert_eq!(v["items"][0]["content_digest"], digest);
}
