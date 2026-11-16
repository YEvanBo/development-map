// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
use crate::store::{limit, Error, Result};
use rusqlite::{params, Transaction};
use serde_json::{json, Value};
// Cursor includes endpoint scope and a lexicographic tuple, avoiding concatenation ambiguity.
fn decode(after: &str, scope: &str) -> Result<(String, String)> {
    if after.is_empty() {
        return Ok((String::new(), String::new()));
    }
    let v: (String, String, String) =
        serde_json::from_str(after).map_err(|_| Error::Invalid("relation cursor"))?;
    if v.0 != scope {
        return Err(Error::Invalid("relation cursor scope"));
    }
    Ok((v.1, v.2))
}
fn page(mut rows: Vec<Value>, n: usize, scope: &str) -> Value {
    let more = rows.len() > n;
    rows.truncate(n);
    let next = if more {
        rows.last().map(|v| {
            serde_json::to_string(&(
                scope,
                v["key1"].as_str().unwrap(),
                v["key2"].as_str().unwrap(),
            ))
            .unwrap()
        })
    } else {
        None
    };
    for v in &mut rows {
        v.as_object_mut().unwrap().remove("key1");
        v.as_object_mut().unwrap().remove("key2");
    }
    json!({"items":rows,"next":next,"has_more":more})
}
pub fn traces(tx: &Transaction<'_>, id: &str, rev: &str, after: &str, n: usize) -> Result<Value> {
    let n = limit(Some(n));
    let scope = serde_json::to_string(&("traces", id, rev)).unwrap();
    let (a, b) = decode(after, &scope)?;
    let mut q=tx.prepare("SELECT n.id,n.title,t.relation,t.source_ref FROM map_local_traces t JOIN map_dev_nodes n ON n.id=t.dev_node_id WHERE t.arch_node_id=? AND t.arch_version=? AND (n.id,t.relation)>(?,?) ORDER BY n.id,t.relation LIMIT ?")?;
    let rows=q.query_map(params![id,rev,a,b,(n+1)as i64],|r|{let id:String=r.get(0)?;let rel:String=r.get(2)?;Ok(json!({"node_id":id,"title":r.get::<_,String>(1)?,"relation":rel,"source_ref":r.get::<_,String>(3)?,"key1":id,"key2":rel}))})?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(rows, n, &scope))
}
pub fn interfaces(
    tx: &Transaction<'_>,
    id: &str,
    rev: &str,
    after: &str,
    n: usize,
) -> Result<Value> {
    let n = limit(Some(n));
    let scope = serde_json::to_string(&("interfaces", id, rev)).unwrap();
    let (a, b) = decode(after, &scope)?;
    let mut q=tx.prepare("SELECT to_id,kind,source_ref FROM map_local_interfaces WHERE from_id=? AND arch_version=? AND (to_id,kind)>(?,?) ORDER BY to_id,kind LIMIT ?")?;
    let rows=q.query_map(params![id,rev,a,b,(n+1)as i64],|r|{let id:String=r.get(0)?;let kind:String=r.get(1)?;Ok(json!({"to_id":id,"kind":kind,"source_ref":r.get::<_,String>(2)?,"key1":id,"key2":kind}))})?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(rows, n, &scope))
}
pub fn search(
    tx: &Transaction<'_>,
    project: &str,
    term: &str,
    after: &str,
    n: usize,
) -> Result<Value> {
    if term.len() < 2 || term.len() > 100 {
        return Err(Error::Invalid("search query"));
    }
    let n = limit(Some(n));
    let scope = serde_json::to_string(&("search", project, term)).unwrap();
    let (a, b) = decode(after, &scope)?;
    let pattern = format!(
        "{}%",
        term.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let mut q=tx.prepare("SELECT id,title,kind,plan_state FROM map_dev_nodes WHERE project_id=? AND (id LIKE ? ESCAPE '\\' OR title LIKE ? ESCAPE '\\') AND (title,id)>(?,?) ORDER BY title,id LIMIT ?")?;
    let rows=q.query_map(params![project,pattern,pattern,a,b,(n+1)as i64],|r|{let id:String=r.get(0)?;let title:String=r.get(1)?;Ok(json!({"id":id,"title":title,"kind":r.get::<_,String>(2)?,"plan_state":r.get::<_,String>(3)?,"key1":title,"key2":id}))})?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(rows, n, &scope))
}
pub fn dependencies(tx: &Transaction<'_>, id: &str, after: &str, n: usize) -> Result<Value> {
    let n = limit(Some(n));
    let scope = serde_json::to_string(&("dependencies", id)).unwrap();
    let (a, b) = decode(after, &scope)?;
    let mut q=tx.prepare("SELECT e.to_id,e.kind FROM map_edges e JOIN map_dev_nodes a ON a.id=e.from_id JOIN map_dev_nodes b ON b.id=e.to_id WHERE e.from_id=? AND a.project_id=b.project_id AND (e.to_id,e.kind)>(?,?) ORDER BY e.to_id,e.kind LIMIT ?")?;
    let rows = q
        .query_map(params![id, a, b, (n + 1) as i64], |r| {
            let to: String = r.get(0)?;
            let kind: String = r.get(1)?;
            Ok(json!({"to_id":to,"kind":kind,"key1":to,"key2":kind}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(rows, n, &scope))
}
pub fn node_traces(
    tx: &Transaction<'_>,
    id: &str,
    rev: &str,
    after: &str,
    n: usize,
) -> Result<Value> {
    let n = limit(Some(n));
    let scope = serde_json::to_string(&("node-traces", id, rev)).unwrap();
    let (a, b) = decode(after, &scope)?;
    let mut q=tx.prepare("SELECT a.id,a.title,t.relation,t.source_ref FROM map_local_traces t JOIN map_arch_nodes a ON a.id=t.arch_node_id AND a.arch_version=t.arch_version WHERE t.dev_node_id=? AND t.arch_version=? AND (a.id,t.relation)>(?,?) ORDER BY a.id,t.relation LIMIT ?")?;
    let rows=q.query_map(params![id,rev,a,b,(n+1)as i64],|r|{let id:String=r.get(0)?;let rel:String=r.get(2)?;Ok(json!({"id":id,"title":r.get::<_,String>(1)?,"relation":rel,"source_ref":r.get::<_,String>(3)?,"key1":id,"key2":rel}))})?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(rows, n, &scope))
}
pub fn checkpoint_artifacts(
    tx: &Transaction<'_>,
    node: &str,
    checkpoint: i64,
    after: i64,
    n: usize,
) -> Result<Value> {
    let found: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM map_checkpoint_index WHERE id=? AND node_id=?)",
        params![checkpoint, node],
        |r| r.get(0),
    )?;
    if !found {
        return Err(Error::Missing);
    }
    let n = limit(Some(n));
    let mut q=tx.prepare("SELECT a.id,a.repo_key,a.revision,a.path,a.kind,a.verification,c.provenance FROM map_local_checkpoint_artifacts c JOIN map_artifacts a ON a.id=c.artifact_id WHERE c.checkpoint_rowid=? AND a.id>? ORDER BY a.id LIMIT ?")?;
    let mut rows=q.query_map(params![checkpoint,after,(n+1)as i64],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"repo_key":r.get::<_,String>(1)?,"revision":r.get::<_,String>(2)?,"path":r.get::<_,String>(3)?,"kind":r.get::<_,String>(4)?,"verification":r.get::<_,String>(5)?,"provenance":r.get::<_,String>(6)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let more = rows.len() > n;
    rows.truncate(n);
    let next = if more {
        rows.last().map(|v| v["id"].clone())
    } else {
        None
    };
    Ok(json!({"items":rows,"next":next,"has_more":more,"checkpoint_id":checkpoint}))
}
