// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
use crate::store::{self, MapStore};
use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use uuid::Uuid;
#[derive(Clone)]
pub struct MapWeb {
    pub store: MapStore,
    pub bearer: String,
    scoped: bool,
    worker_token: Option<String>,
    manager_token: Option<String>,
    grants: Arc<Mutex<HashMap<String, Instant>>>,
    launch: Arc<Mutex<Option<(String, Instant)>>>,
}
impl MapWeb {
    pub fn new(store: MapStore, bearer: String) -> Self {
        Self {
            store,
            bearer,
            scoped: false,
            worker_token: None,
            manager_token: None,
            grants: Arc::new(Mutex::new(HashMap::new())),
            launch: Arc::new(Mutex::new(None)),
        }
    }
    pub fn with_scoped_identities(mut self) -> Self {
        self.scoped = true;
        self.worker_token = None;
        self.manager_token = None;
        self
    }
    fn worker_submission(&self, h: &HeaderMap, input: &crate::submission::Submission) -> Response {
        if self.scoped {
            mutation(self.store.submit_bound(input, Some(bearer(h))))
        } else {
            mutation(self.store.submit_legacy_worker(input))
        }
    }
    fn scope(&self, h: &HeaderMap, project: Option<&str>, node: Option<&str>, role: &str) -> bool {
        if !self.scoped {
            return self.write_authorized(h, role == "manager");
        }
        self.store
            .read(|tx| {
                let project = match project {
                    Some(p) => p.to_string(),
                    None => tx.query_row(
                        "SELECT project_id FROM map_dev_nodes WHERE id=?",
                        [node.unwrap_or("")],
                        |r| r.get::<_, String>(0),
                    )?,
                };
                crate::identity::authorize(tx, bearer(h), &project, node, role).map(|_| ())
            })
            .is_ok()
    }
    pub fn with_write_tokens(
        mut self,
        worker: String,
        manager: String,
    ) -> Result<Self, &'static str> {
        if worker.len() < 24
            || manager.len() < 24
            || worker == manager
            || worker == self.bearer
            || manager == self.bearer
        {
            return Err("distinct 24+ character tokens required");
        };
        self.worker_token = Some(worker);
        self.manager_token = Some(manager);
        Ok(self)
    }
    fn write_authorized(&self, h: &HeaderMap, manager: bool) -> bool {
        let supplied = h
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|x| x.strip_prefix("Bearer "))
            .unwrap_or("");
        let expected = if manager {
            self.manager_token.as_deref()
        } else {
            self.worker_token.as_deref()
        };
        expected.is_some_and(|token| {
            supplied.len() == token.len() && bool::from(supplied.as_bytes().ct_eq(token.as_bytes()))
        })
    }
    pub fn issue_launch_code(&self) -> String {
        let code = Uuid::new_v4().to_string();
        *self.launch.lock().unwrap() = Some((code.clone(), Instant::now()));
        code
    }
    pub fn router(self) -> Router {
        let guard_state = self.clone();
        Router::new()
            .route("/map/session", post(session))
            .route("/v1/map/projects", get(projects))
            .route("/v1/map/nodes/{id}/local-files", get(local_files))
            .route("/v1/map/local-source/{operation}", get(local_source))
            .route("/v1/map/architecture", get(architecture))
            .route("/v1/map/architecture/children", get(architecture_children))
            .route("/v1/map/architecture/{id}/traces", get(architecture_traces))
            .route("/v1/map/architecture/{id}/interfaces", get(interfaces))
            .route(
                "/v1/map/architecture/{id}/neighborhood",
                get(architecture_neighborhood),
            )
            .route("/v1/map/children", get(children))
            .route("/v1/map/navigation", get(navigation))
            .route("/v1/map/ancestors", get(ancestors))
            .route("/v1/map/summary", get(summary))
            .route("/v1/map/submissions", post(submit_bundle))
            .route("/v1/map/nodes/{id}", get(node))
            .route("/v1/map/nodes/{id}/resources", get(resources))
            .route("/v1/map/nodes/{id}/directory", get(resource_directory))
            .route("/v1/map/resources/lookup", get(resource_lookup))
            .route("/v1/map/nodes/{id}/checkpoints", get(checkpoints))
            .route("/v1/map/nodes/{id}/artifacts", get(artifacts))
            .route("/v1/map/nodes/{id}/dependencies", get(dependencies))
            .route("/v1/map/nodes/{id}/architecture-traces", get(node_traces))
            .route(
                "/v1/map/nodes/{id}/checkpoint-artifacts",
                get(checkpoint_artifacts),
            )
            .route("/v1/map/artifacts/lookup", get(lookup))
            .route("/v1/map/changes", get(changes))
            .route("/v1/map/context", get(context))
            .route("/v1/map/search", get(search))
            .route("/v1/map/checkpoints", post(submit_checkpoint))
            .route("/v1/map/acceptances", post(submit_acceptance))
            .route("/v1/map/artifacts", post(submit_artifact))
            .route("/v1/map/nodes", post(upsert_node))
            .route("/v1/map/handoffs", post(handoff))
            .route("/v1/map/architecture/adopt", post(adopt_architecture))
            .route("/v1/map/architecture/traces", post(submit_trace))
            .layer(DefaultBodyLimit::max(65536))
            .with_state(self)
            .layer(axum::middleware::from_fn_with_state(
                guard_state,
                scope_guard,
            ))
    }
    fn authorized(&self, h: &HeaderMap) -> bool {
        if self.scoped {
            return self
                .store
                .read(|tx| {
                    let encoded: String = tx.query_row(
                        "SELECT binding FROM map_identities WHERE token_hash=? AND revoked=0",
                        [crate::identity::token_hash(bearer(h))],
                        |r| r.get(0),
                    )?;
                    let b: crate::identity::Binding = serde_json::from_str(&encoded)
                        .map_err(|_| store::Error::Invalid("identity"))?;
                    crate::identity::authorize(
                        tx,
                        bearer(h),
                        &b.project_id,
                        b.node_id.as_deref(),
                        "reader",
                    )
                    .map(|_| ())
                })
                .is_ok();
        }

        let b = h
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|x| x.strip_prefix("Bearer "))
            .unwrap_or("");
        if !self.bearer.is_empty()
            && b.len() == self.bearer.len()
            && bool::from(b.as_bytes().ct_eq(self.bearer.as_bytes()))
        {
            return true;
        }
        let cookie = h
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let value = cookie
            .split(';')
            .filter_map(|x| x.trim().strip_prefix("map_session="))
            .next()
            .unwrap_or("");
        let mut g = self.grants.lock().unwrap();
        g.retain(|_, t| t.elapsed() < Duration::from_secs(900));
        g.contains_key(value) && !value.is_empty()
    }
}
fn fail(e: store::Error) -> Response {
    let (code, msg) = match e {
        store::Error::Invalid(x) => (StatusCode::BAD_REQUEST, x.to_string()),
        store::Error::Conflict(x) => (StatusCode::CONFLICT, x.to_string()),
        store::Error::Missing => (StatusCode::NOT_FOUND, "missing".to_string()),
        store::Error::Db(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "database operation failed".to_string(),
        ),
    };
    (code, Json(json!({"error":msg}))).into_response()
}
fn result(
    web: &MapWeb,
    h: &HeaderMap,
    f: impl FnOnce(&rusqlite::Transaction<'_>) -> store::Result<Value>,
) -> Response {
    if !web.authorized(h) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut forbidden = false;
    let mut missing = false;
    let response = web.store.read(|tx| {
        if web.scoped {
            match SCOPED_READ_URI.try_with(|uri| authorize_read(tx, h, uri)) {
                Ok(Ok(())) => {}
                Ok(Err(store::Error::Missing)) => {
                    missing = true;
                    return Err(store::Error::Missing);
                }
                Ok(Err(_)) | Err(_) => {
                    forbidden = true;
                    return Err(store::Error::Invalid("read scope"));
                }
            }
        }
        f(tx)
    });
    if missing {
        return StatusCode::NOT_FOUND.into_response();
    }
    if forbidden {
        return StatusCode::FORBIDDEN.into_response();
    }
    match response {
        Ok(v) => {
            let size = serde_json::to_vec(&v)
                .map(|b| b.len())
                .unwrap_or(usize::MAX);
            if size > 65536 {
                (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(json!({"error":"response exceeds 64 KiB; request a smaller page"})),
                )
                    .into_response()
            } else {
                Json(v).into_response()
            }
        }
        Err(e) => fail(e),
    }
}
fn cap(q: &HashMap<String, String>) -> usize {
    store::limit(q.get("limit").and_then(|x| x.parse().ok()))
}
fn number(q: &HashMap<String, String>, key: &str) -> i64 {
    q.get(key)
        .and_then(|x| x.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0)
}
async fn projects(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if w.scoped {
        return result(&w, &h, |tx| {
            let encoded: String = tx.query_row(
                "SELECT binding FROM map_identities WHERE token_hash=? AND revoked=0",
                [crate::identity::token_hash(bearer(&h))],
                |r| r.get(0),
            )?;
            let b: crate::identity::Binding =
                serde_json::from_str(&encoded).map_err(|_| store::Error::Invalid("identity"))?;
            let v=tx.query_row("SELECT id,name,source_ref,(SELECT COUNT(*) FROM map_dev_nodes WHERE project_id=map_projects.id) FROM map_projects WHERE id=?",[b.project_id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"source_ref":r.get::<_,String>(2)?,"node_count":r.get::<_,i64>(3)?})))?;
            Ok(json!({"items":[v],"next":null}))
        });
    }
    result(&w, &h, |tx| {
        store::list_projects(
            tx,
            q.get("after").map(String::as_str).unwrap_or(""),
            cap(&q),
        )
    })
}
async fn children(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let parent = q.get("parent_ref").or_else(|| q.get("parent_id"));
    let Some(p) = parent else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if let Some(project) = q.get("project_id") {
        // `parent_ref` is a typed reference; `parent_id` is always a raw dev-node
        // id (which may itself contain colons). Never reinterpret one as the
        // other: the navigation projection keys on `dev:<id>`.
        let parent = if q.contains_key("parent_ref") {
            p.to_string()
        } else {
            format!("dev:{p}")
        };
        result(&w, &h, |tx| {
            crate::navigation::children(
                tx,
                project,
                q.get("revision").map(String::as_str),
                &parent,
                q.get("after").map(String::as_str).unwrap_or(""),
                cap(&q),
                q.get("state").map(String::as_str),
            )
        })
    } else if q.contains_key("parent_ref") {
        result(&w, &h, |tx| {
            store::navigation_children(
                tx,
                p,
                q.get("after").map(String::as_str).unwrap_or(""),
                cap(&q),
            )
        })
    } else {
        // A bare `parent_id` is passed through as the raw dev-node id. Stripping
        // a `dev:` prefix here would let `dev:project:q` be re-interpreted by
        // store::children as project `q` after authorization used a different id.
        result(&w, &h, |tx| {
            store::children_of_node(
                tx,
                p,
                q.get("after").map(String::as_str).unwrap_or(""),
                cap(&q),
                q.get("state").map(String::as_str),
            )
        })
    }
}
async fn node(State(w): State<MapWeb>, h: HeaderMap, Path(id): Path<String>) -> Response {
    result(&w, &h, |tx| store::node_detail(tx, &id))
}
async fn checkpoints(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    result(&w, &h, |tx| {
        store::checkpoints(tx, &id, number(&q, "after"), cap(&q))
    })
}
async fn artifacts(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    result(&w, &h, |tx| {
        store::artifacts(
            tx,
            &id,
            number(&q, "after"),
            cap(&q),
            q.get("kind").map(String::as_str),
        )
    })
}
async fn lookup(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let (Some(repo), Some(rev), Some(path)) = (q.get("repo_key"), q.get("revision"), q.get("path"))
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    result(&w, &h, |tx| {
        let after = q.get("after").map(String::as_str).unwrap_or("");
        let (aid, node) = after.split_once(':').unwrap_or(("0", ""));
        let aid = aid.parse::<i64>().unwrap_or(0);
        store::lookup_page(tx, repo, rev, path, aid, node, cap(&q))
    })
}
async fn changes(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Some(p) = q.get("project_id") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    result(&w, &h, |tx| {
        store::changes(tx, p, number(&q, "after_event_id"), cap(&q))
    })
}
async fn context(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Some(id) = q.get("node_id") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    result(&w, &h, |tx| {
        let node = store::node_detail(tx, id)?;
        let artifacts = store::artifacts(tx, id, 0, 20, None)?;
        Ok(json!({"node":node,"artifacts":artifacts}))
    })
}
#[derive(Deserialize)]
struct Launch {
    code: String,
}
async fn session(State(w): State<MapWeb>, h: HeaderMap, Json(input): Json<Launch>) -> Response {
    let origin = h
        .get(header::ORIGIN)
        .and_then(|x| x.to_str().ok())
        .unwrap_or("");
    let host = h
        .get(header::HOST)
        .and_then(|x| x.to_str().ok())
        .unwrap_or("");
    if origin != format!("http://{host}")
        || !(host.starts_with("127.0.0.1:") || host.starts_with("localhost:"))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut launch = w.launch.lock().unwrap();
    if !launch
        .as_ref()
        .is_some_and(|(code, time)| code == &input.code && time.elapsed() < Duration::from_secs(60))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    *launch = None;
    let token = Uuid::new_v4().to_string();
    w.grants
        .lock()
        .unwrap()
        .insert(token.clone(), Instant::now());
    let mut r = Json(json!({"ok":true})).into_response();
    r.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "map_session={token}; HttpOnly; SameSite=Strict; Path=/map; Max-Age=900"
        ))
        .unwrap(),
    );
    r.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "map_session={token}; HttpOnly; SameSite=Strict; Path=/v1/map; Max-Age=900"
        ))
        .unwrap(),
    );
    r
}
async fn architecture(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Some(p) = q.get("project_id") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    result(&w, &h, |tx| {
        store::architecture_revision(tx, p, q.get("revision").map(String::as_str))
    })
}
async fn architecture_children(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Some(p) = q.get("project_id") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    result(&w, &h, |tx| {
        store::architecture_children_revision(
            tx,
            p,
            q.get("parent_id").map(String::as_str),
            q.get("after").map(String::as_str).unwrap_or(""),
            cap(&q),
            q.get("revision").map(String::as_str),
        )
    })
}
async fn architecture_traces(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    result(&w, &h, |tx| {
        let project = store::arch_project(tx, &id, q.get("revision").map(String::as_str))?;
        let rev = crate::navigation::revision(tx, &project, q.get("revision").map(String::as_str))?;
        crate::relations::traces(
            tx,
            &id,
            &rev,
            q.get("after").map(String::as_str).unwrap_or(""),
            cap(&q),
        )
    })
}

async fn architecture_neighborhood(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    result(&w, &h, |tx| {
        store::architecture_neighborhood_revision(
            tx,
            &id,
            cap(&q),
            q.get("revision").map(String::as_str),
        )
    })
}

async fn search(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let (Some(p), Some(term)) = (q.get("project_id"), q.get("q")) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    result(&w, &h, |tx| {
        crate::relations::search(
            tx,
            p,
            term,
            q.get("after").map(String::as_str).unwrap_or(""),
            cap(&q),
        )
    })
}

#[derive(Deserialize)]
struct AcceptInput {
    node_id: String,
    generation: i64,
    version: i64,
    digest: String,
    decision: String,
    evidence_level: String,
    source_ref: String,
}
#[derive(Deserialize)]
struct AdoptInput {
    project_id: String,
    version: String,
    source_ref: String,
    digest: String,
    variant: String,
    adoption_ref: String,
}
#[derive(Deserialize)]
struct TraceInput {
    arch_node_id: String,
    dev_node_id: String,
    relation: String,
    source_ref: String,
}
fn mutation(r: store::Result<impl serde::Serialize>) -> Response {
    match r {
        Ok(v) => Json(json!({"ok":true,"result":v})).into_response(),
        Err(e) => fail(e),
    }
}
async fn submit_checkpoint(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Json(input): Json<store::CheckpointInput>,
) -> Response {
    if !w.scope(&h, None, Some(&input.node_id), "worker") {
        return StatusCode::FORBIDDEN.into_response();
    }
    let bundle = crate::submission::Submission {
        base_checkpoint_rowid: match h.get("x-map-base-checkpoint") {
            Some(value) => match value.to_str().ok().and_then(|v| v.parse::<i64>().ok()) {
                Some(base) => Some(base),
                None => return StatusCode::BAD_REQUEST.into_response(),
            },
            None => None,
        },
        submission_key: format!(
            "checkpoint:{}:{}:{}",
            input.node_id, input.generation, input.version
        ),
        checkpoint: input,
        artifacts: vec![],
        resources: None,
    };
    w.worker_submission(&h, &bundle)
}
async fn submit_acceptance(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Json(x): Json<AcceptInput>,
) -> Response {
    if !w.scope(&h, None, Some(&x.node_id), "manager") {
        return StatusCode::FORBIDDEN.into_response();
    }
    mutation(w.store.accept_bound(
        &x.node_id,
        x.generation,
        x.version,
        &x.digest,
        &x.decision,
        &x.evidence_level,
        &x.source_ref,
        w.scoped.then(|| bearer(&h)),
    ))
}
async fn submit_artifact(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Json(input): Json<store::ArtifactInput>,
) -> Response {
    if w.scoped || !w.write_authorized(&h, false) {
        return StatusCode::FORBIDDEN.into_response();
    }
    mutation(w.store.legacy_worker_artifact(&input))
}
async fn upsert_node(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Json(input): Json<store::NodeInput>,
) -> Response {
    if !w.scope(&h, Some(&input.project_id), Some(&input.id), "manager") {
        return StatusCode::FORBIDDEN.into_response();
    }
    mutation(w.store.node_bound(&input, w.scoped.then(|| bearer(&h))))
}
async fn adopt_architecture(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Json(x): Json<AdoptInput>,
) -> Response {
    if !w.scope(&h, Some(&x.project_id), None, "manager") {
        return StatusCode::FORBIDDEN.into_response();
    }
    mutation(w.store.adopt_bound(
        &x.project_id,
        &x.version,
        &x.source_ref,
        &x.digest,
        &x.variant,
        &x.adoption_ref,
        w.scoped.then(|| bearer(&h)),
    ))
}
async fn submit_trace(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Json(x): Json<TraceInput>,
) -> Response {
    if !w.scope(&h, None, Some(&x.dev_node_id), "manager") {
        return StatusCode::FORBIDDEN.into_response();
    }
    mutation(w.store.trace_bound(
        &x.arch_node_id,
        &x.dev_node_id,
        &x.relation,
        &x.source_ref,
        w.scoped.then(|| bearer(&h)),
    ))
}

async fn navigation(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let (Some(p), Some(n)) = (q.get("project_id"), q.get("node_ref")) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    result(&w, &h, |tx| {
        crate::navigation::focus(tx, p, q.get("revision").map(String::as_str), n)
    })
}
async fn summary(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let (Some(p), Some(n)) = (q.get("project_id"), q.get("node_ref")) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    result(&w, &h, |tx| {
        crate::navigation::summary(tx, p, q.get("revision").map(String::as_str), n)
    })
}
async fn ancestors(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let (Some(p), Some(n)) = (q.get("project_id"), q.get("node_ref")) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    result(&w, &h, |tx| {
        let rev = crate::navigation::revision(tx, p, q.get("revision").map(String::as_str))?;
        crate::navigation::ancestors(tx, p, &rev, n, q.get("after").map(String::as_str), cap(&q))
    })
}
async fn submit_bundle(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Json(input): Json<crate::submission::Submission>,
) -> Response {
    if !w.scope(&h, None, Some(&input.checkpoint.node_id), "worker") {
        return StatusCode::FORBIDDEN.into_response();
    }
    w.worker_submission(&h, &input)
}

fn bearer(h: &HeaderMap) -> &str {
    h.get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
}

async fn interfaces(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    result(&w, &h, |tx| {
        let project = store::arch_project(tx, &id, q.get("revision").map(String::as_str))?;
        let rev = crate::navigation::revision(tx, &project, q.get("revision").map(String::as_str))?;
        crate::relations::interfaces(
            tx,
            &id,
            &rev,
            q.get("after").map(String::as_str).unwrap_or(""),
            cap(&q),
        )
    })
}

async fn scope_guard(
    State(w): State<MapWeb>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if !w.scoped {
        return next.run(req).await;
    }
    let path = req.uri().path();
    if path == "/map/session" {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !path.starts_with("/v1/map/") {
        return next.run(req).await;
    }
    // Missing, expired, revoked or wrong-token identities are 401 on every
    // scoped map route, including writes. Role/project scope remains 403.
    if !w.authorized(req.headers()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !matches!(
        *req.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    ) {
        return next.run(req).await;
    }
    let uri = req.uri().clone();
    SCOPED_READ_URI.scope(uri, next.run(req)).await
}

tokio::task_local! {
    static SCOPED_READ_URI: axum::http::Uri;
}

fn authorize_read(
    tx: &rusqlite::Transaction<'_>,
    h: &HeaderMap,
    uri: &axum::http::Uri,
) -> store::Result<()> {
    let path = uri.path();
    if path == "/v1/map/projects" {
        let encoded: String = tx.query_row(
            "SELECT binding FROM map_identities WHERE token_hash=? AND revoked=0",
            [crate::identity::token_hash(bearer(h))],
            |row| row.get(0),
        )?;
        let binding: crate::identity::Binding =
            serde_json::from_str(&encoded).map_err(|_| store::Error::Invalid("identity"))?;
        return crate::identity::authorize(
            tx,
            bearer(h),
            &binding.project_id,
            binding.node_id.as_deref(),
            "reader",
        )
        .map(|_| ());
    }
    let q = Query::<HashMap<String, String>>::try_from_uri(uri)
        .map_err(|_| store::Error::Invalid("invalid query"))?
        .0;
    // Authorize the selector consumed by this handler. Unrelated query
    // parameters must never supply an alternate authorization target.
    let decode = |s: &str| {
        percent_encoding::percent_decode_str(s)
            .decode_utf8()
            .map(|v| v.into_owned())
            .map_err(|_| store::Error::Invalid("path encoding"))
    };
    let mut node = None;
    let mut arch = None;
    let mut ref_project = None;
    if let Some(tail) = path.strip_prefix("/v1/map/nodes/") {
        node = Some(decode(tail.split('/').next().unwrap_or(""))?);
    } else if let Some(tail) = path.strip_prefix("/v1/map/architecture/") {
        if tail == "children" {
            arch = q.get("parent_id").cloned();
        } else {
            arch = Some(decode(tail.split('/').next().unwrap_or(""))?);
        }
    } else if path == "/v1/map/context" {
        node = q.get("node_id").cloned();
    } else if path == "/v1/map/children" {
        if let Some(selector) = q.get("parent_ref") {
            // `parent_ref` is a typed reference: dev:/arch:/project:.
            if let Some(id) = selector.strip_prefix("dev:") {
                node = Some(id.into());
            } else if let Some(id) = selector.strip_prefix("arch:") {
                arch = Some(id.into());
            } else if let Some(id) = selector.strip_prefix("project:") {
                ref_project = Some(id.to_string());
            }
        } else {
            // `parent_id` is a raw dev-node id; never strip a prefix here or the
            // authorization target and store lookup can diverge.
            let raw = q
                .get("parent_id")
                .ok_or(store::Error::Invalid("node selector required"))?;
            node = Some(raw.clone());
        }
    } else if matches!(
        path,
        "/v1/map/navigation" | "/v1/map/summary" | "/v1/map/ancestors"
    ) {
        let selector = q
            .get("node_ref")
            .ok_or(store::Error::Invalid("node selector required"))?;
        if let Some(id) = selector.strip_prefix("dev:") {
            node = Some(id.into());
        } else if let Some(id) = selector.strip_prefix("arch:") {
            arch = Some(id.into());
        } else if let Some(id) = selector.strip_prefix("project:") {
            ref_project = Some(id.to_string());
        }
    } else if path == "/v1/map/local-source/history" {
        node = q.get("node_id").cloned();
    } else if !matches!(
        path,
        "/v1/map/search"
            | "/v1/map/architecture"
            | "/v1/map/changes"
            | "/v1/map/resources/lookup"
            | "/v1/map/local-source/status"
            | "/v1/map/local-source/file"
            | "/v1/map/local-source/tree"
            | "/v1/map/local-source/lookup"
    ) {
        return Err(store::Error::Invalid("unrecognized read scope"));
    }
    let mut resource_missing = false;
    let actual: Option<String> = if let Some(n) = &node {
        match tx
            .query_row(
                "SELECT project_id FROM map_dev_nodes WHERE id=?",
                [n],
                |r| r.get(0),
            )
            .optional()?
        {
            Some(project) => Some(project),
            None => {
                resource_missing = true;
                None
            }
        }
    } else if let Some(a) = &arch {
        match store::arch_project(tx, a, q.get("revision").map(String::as_str)) {
            Ok(project) => Some(project),
            Err(store::Error::Missing) => {
                resource_missing = true;
                None
            }
            Err(e) => return Err(e),
        }
    } else {
        None
    };
    let resource_selected = node.is_some() || arch.is_some() || ref_project.is_some();
    if resource_selected {
        // Path/reference-selected resources never reveal whether an id exists
        // in another project: missing, out-of-scope and project-mismatch all
        // collapse to 404. 401 (identity) remains the middleware's job.
        if let Some(requested) = q.get("project_id") {
            let mismatch = actual.as_deref().is_some_and(|a| a != requested)
                || ref_project.as_deref().is_some_and(|r| r != requested);
            if mismatch {
                return Err(store::Error::Missing);
            }
        }
        if let Some(project) = actual.as_deref().or(ref_project.as_deref()) {
            if crate::identity::authorize(tx, bearer(h), project, node.as_deref(), "reader")
                .is_err()
            {
                return Err(store::Error::Missing);
            }
        }
        if resource_missing {
            return Err(store::Error::Missing);
        }
        return Ok(());
    }
    // Project-scoped collections/pages keep 403 for an out-of-scope project.
    let project = q
        .get("project_id")
        .cloned()
        .or_else(|| {
            q.get("parent_id")
                .and_then(|p| p.strip_prefix("project:").map(str::to_string))
        })
        .ok_or(store::Error::Invalid("project required"))?;
    crate::identity::authorize(tx, bearer(h), &project, None, "reader")?;
    Ok(())
}

async fn dependencies(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    result(&w, &h, |tx| {
        crate::relations::dependencies(
            tx,
            &id,
            q.get("after").map(String::as_str).unwrap_or(""),
            cap(&q),
        )
    })
}
async fn node_traces(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    result(&w, &h, |tx| {
        let project: String = tx.query_row(
            "SELECT project_id FROM map_dev_nodes WHERE id=?",
            [&id],
            |r| r.get(0),
        )?;
        let rev = crate::navigation::revision(tx, &project, q.get("revision").map(String::as_str))?;
        crate::relations::node_traces(
            tx,
            &id,
            &rev,
            q.get("after").map(String::as_str).unwrap_or(""),
            cap(&q),
        )
    })
}
async fn checkpoint_artifacts(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    result(&w, &h, |tx| {
        crate::relations::checkpoint_artifacts(
            tx,
            &id,
            number(&q, "checkpoint_id"),
            number(&q, "after"),
            cap(&q),
        )
    })
}

async fn handoff(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Json(input): Json<crate::handoff::Handoff>,
) -> Response {
    if !w.scope(&h, None, Some(&input.node_id), "manager") {
        return StatusCode::FORBIDDEN.into_response();
    }
    mutation(w.store.handoff(&input, w.scoped.then(|| bearer(&h))))
}

async fn resources(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    result(&w, &h, |tx| {
        let checkpoint = q
            .get("checkpoint_id")
            .map(|v| {
                v.parse::<i64>()
                    .map_err(|_| store::Error::Invalid("checkpoint ID"))
            })
            .transpose()?;
        crate::resources::page(
            tx,
            &id,
            checkpoint,
            q.get("after").map(String::as_str).unwrap_or(""),
            cap(&q),
        )
    })
}
async fn resource_directory(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    result(&w, &h, |tx| {
        crate::resources::directory(
            tx,
            &id,
            number(&q, "checkpoint_id"),
            q.get("key").map(String::as_str).unwrap_or(""),
            q.get("path").map(String::as_str).unwrap_or(""),
            q.get("after").map(String::as_str).unwrap_or(""),
            cap(&q),
        )
    })
}
async fn resource_lookup(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let value = |key| q.get(key).map(String::as_str).unwrap_or("");
    result(&w, &h, |tx| {
        crate::resources::lookup(
            tx,
            value("project_id"),
            value("repo_key"),
            value("revision"),
            value("path"),
            value("after"),
            cap(&q),
        )
    })
}

async fn local_files(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    result(&w, &h, |tx| {
        crate::local_source::files(
            tx,
            &id,
            q.get("after").map(String::as_str).unwrap_or(""),
            cap(&q).min(20),
        )
    })
}
async fn local_source(
    State(w): State<MapWeb>,
    h: HeaderMap,
    Path(operation): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let v = |key| q.get(key).map(String::as_str).unwrap_or("");
    result(&w, &h, |tx| match operation.as_str() {
        "status" => crate::local_source::status(tx, v("project_id")),
        "file" => crate::local_source::file(tx, v("project_id"), v("path")),
        "tree" => {
            crate::local_source::tree(tx, v("project_id"), v("path"), v("after"), cap(&q).min(20))
        }
        "lookup" => {
            crate::local_source::lookup(tx, v("project_id"), v("path"), v("after"), cap(&q))
        }
        "history" => {
            let node = q.get("node_id").map(String::as_str);
            let project = match q.get("project_id") {
                Some(p) => p.to_string(),
                None => match node {
                    Some(n) => tx
                        .query_row(
                            "SELECT project_id FROM map_dev_nodes WHERE id=?",
                            [n],
                            |r| r.get::<_, String>(0),
                        )
                        .optional()?
                        .unwrap_or_default(),
                    None => String::new(),
                },
            };
            crate::local_source::history(tx, &project, node, number(&q, "after"), cap(&q).min(10))
        }
        _ => Err(store::Error::Missing),
    })
}
