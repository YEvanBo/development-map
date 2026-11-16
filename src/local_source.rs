// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
//! Read-only local-source projections. Never creates execution identities, checkpoints or acceptances.
use crate::store::{event, Error, MapStore, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub project_id: String,
    pub name: String,
    pub core_node: String,
    pub source_root: String,
    pub config_digest: String,
    pub digest: String,
    pub observed_at: i64,
    pub stale_after_seconds: i64,
    pub nodes: Vec<Node>,
    pub files: Vec<Value>,
    pub links: Vec<Link>,
    #[serde(flatten)]
    pub metadata: serde_json::Map<String, Value>,
}
#[derive(Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub kind: String,
    pub details: Value,
}
#[derive(Serialize, Deserialize)]
pub struct Link {
    pub node_id: String,
    pub path: String,
    pub category: String,
}
pub fn semantic_digest(s: &Snapshot) -> Result<String> {
    let mut semantic = serde_json::to_value(s).map_err(|_| Error::Invalid("snapshot JSON"))?;
    semantic.as_object_mut().unwrap().remove("digest");
    semantic.as_object_mut().unwrap().remove("observed_at");
    if let Some(sources) = semantic["sources"].as_array_mut() {
        for item in sources {
            item.as_object_mut()
                .ok_or(Error::Invalid("source record"))?
                .remove("mtime");
        }
    }
    if let Some(files) = semantic["files"].as_array_mut() {
        for item in files {
            item.as_object_mut()
                .ok_or(Error::Invalid("file record"))?
                .remove("source_mtime");
        }
    }
    use sha2::{Digest, Sha256};
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&semantic).unwrap())
    ))
}
impl MapStore {
    pub fn import_local_source(&self, s: &Snapshot) -> Result<Value> {
        if s.project_id.is_empty()
            || s.digest.len() != 64
            || s.nodes.len() > 3000
            || s.files.len() > 2000
            || s.links.len() > 10000
            || serde_json::to_vec(s).unwrap().len() > 12 * 1024 * 1024
        {
            return Err(Error::Invalid("local snapshot bounds"));
        }
        let actual = semantic_digest(s)?;
        if actual != s.digest {
            return Err(Error::Invalid("snapshot content digest mismatch"));
        }
        let ids = s
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if ids.len() != s.nodes.len() || !ids.contains(s.core_node.as_str()) {
            return Err(Error::Invalid("local node identities"));
        }
        for n in &s.nodes {
            if !n.id.starts_with(&format!("{}/", s.project_id))
                || n.title.len() > 1000
                || !matches!(
                    n.kind.as_str(),
                    "domain" | "module" | "submodule" | "function" | "work_package"
                )
                || n.parent_id.as_deref().is_some_and(|p| !ids.contains(p))
            {
                return Err(Error::Invalid("local hierarchy"));
            }
            let mut seen = std::collections::HashSet::new();
            let mut cursor = Some(n.id.as_str());
            while let Some(id) = cursor {
                if !seen.insert(id) {
                    return Err(Error::Invalid("local hierarchy cycle"));
                }
                cursor = s
                    .nodes
                    .iter()
                    .find(|n| n.id == id)
                    .and_then(|n| n.parent_id.as_deref());
            }
        }
        let paths = s
            .files
            .iter()
            .filter_map(|f| f["path"].as_str())
            .collect::<std::collections::HashSet<_>>();
        if paths.len() != s.files.len() {
            return Err(Error::Invalid("local file identities"));
        }
        for path in &paths {
            crate::resources::valid_path(path)?;
        }
        if s.links
            .iter()
            .any(|l| !ids.contains(l.node_id.as_str()) || !paths.contains(l.path.as_str()))
        {
            return Err(Error::Invalid("local resource reference"));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let old: Option<(String, i64)> = tx
            .query_row(
                "SELECT digest,version FROM map_local_sources WHERE project_id=?",
                [&s.project_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((digest, version)) = &old {
            if digest == &s.digest {
                tx.execute(
                    "UPDATE map_local_sources SET checked_at=?,last_error=NULL WHERE project_id=?",
                    params![s.observed_at, s.project_id],
                )?;
                tx.commit()?;
                return Ok(json!({"changed":false,"version":version,"digest":digest}));
            }
        }
        // A project may be onboarded incrementally: imported source nodes remain
        // read-only while separately named managed nodes use the native result
        // path.  The collision check below is the boundary that prevents a
        // refresh from overwriting an assigned node or its checkpoints.
        for n in &s.nodes {
            let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM map_dev_nodes n WHERE n.id=? AND (n.project_id<>? OR n.owner IS NOT NULL OR EXISTS(SELECT 1 FROM map_checkpoint_index c WHERE c.node_id=n.id) OR NOT EXISTS(SELECT 1 FROM map_local_nodes l WHERE l.node_id=n.id)))",params![n.id,s.project_id],|r|r.get(0))?;
            if exists {
                return Err(Error::Conflict(
                    "local snapshot would overwrite managed node",
                ));
            }
        }
        let version = old.map(|(_, v)| v + 1).unwrap_or(1);
        let metadata = json!({"metadata":s.metadata,"core_node":s.core_node,"source_root":s.source_root,"config_digest":s.config_digest,"node_count":s.nodes.len(),"file_count":s.files.len()});
        tx.execute("INSERT INTO map_projects VALUES(?,?,?,unixepoch()) ON CONFLICT(id) DO UPDATE SET name=excluded.name,source_ref=excluded.source_ref,updated_at=excluded.updated_at",params![s.project_id,s.name,"local-source:unversioned-read-only"])?;
        tx.execute("UPDATE map_dev_nodes SET parent_id=NULL WHERE id IN(SELECT node_id FROM map_local_nodes WHERE project_id=?)",[&s.project_id])?;
        tx.execute("DELETE FROM map_edges WHERE from_id IN(SELECT node_id FROM map_local_nodes WHERE project_id=?)",[&s.project_id])?;
        let old_ids = {
            let mut q = tx.prepare("SELECT node_id FROM map_local_nodes WHERE project_id=?")?;
            let rows = q
                .query_map([&s.project_id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        tx.execute(
            "DELETE FROM map_local_nodes WHERE project_id=?",
            [&s.project_id],
        )?;
        tx.execute(
            "DELETE FROM map_local_links WHERE project_id=?",
            [&s.project_id],
        )?;
        tx.execute(
            "DELETE FROM map_local_files WHERE project_id=?",
            [&s.project_id],
        )?;
        for id in old_ids {
            if !ids.contains(id.as_str()) {
                tx.execute("DELETE FROM map_dev_nodes WHERE id=?", [id])?;
            }
        }
        for (i, n) in s.nodes.iter().enumerate() {
            let plan = format!(
                "source:{}",
                n.details["source_status"].as_str().unwrap_or("reference")
            );
            tx.execute("INSERT INTO map_dev_nodes(id,project_id,parent_id,kind,title,goal,sort_key,plan_state,architecture_ref,owner,generation,updated_at) VALUES(?,?,NULL,?,?,?,?,?,?,NULL,0,unixepoch()) ON CONFLICT(id) DO UPDATE SET kind=excluded.kind,title=excluded.title,goal=excluded.goal,sort_key=excluded.sort_key,plan_state=excluded.plan_state,architecture_ref=excluded.architecture_ref,updated_at=excluded.updated_at",params![n.id,s.project_id,n.kind,n.title,n.details["goal"].as_str().unwrap_or(""),format!("{i:05}"),plan,n.details["source_ref"].as_str()])?;
            tx.execute(
                "INSERT INTO map_local_nodes VALUES(?,?,?)",
                params![
                    n.id,
                    s.project_id,
                    serde_json::to_string(&n.details).unwrap()
                ],
            )?;
        }
        for n in &s.nodes {
            tx.execute(
                "UPDATE map_dev_nodes SET parent_id=? WHERE id=?",
                params![n.parent_id, n.id],
            )?;
            if let Some(deps) = n.details["dependencies"].as_array() {
                for to in deps
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|id| ids.contains(id))
                {
                    tx.execute(
                        "INSERT OR IGNORE INTO map_edges VALUES(?,?,'depends_on')",
                        params![n.id, to],
                    )?;
                }
            }
        }
        for f in &s.files {
            tx.execute(
                "INSERT INTO map_local_files VALUES(?,?,?)",
                params![
                    s.project_id,
                    f["path"].as_str().unwrap(),
                    serde_json::to_string(f).unwrap()
                ],
            )?;
        }
        for l in &s.links {
            tx.execute(
                "INSERT INTO map_local_links VALUES(?,?,?,?)",
                params![s.project_id, l.node_id, l.path, l.category],
            )?;
        }
        tx.execute("INSERT INTO map_local_sources VALUES(?,?,?,?,?,?,?,NULL,?) ON CONFLICT(project_id) DO UPDATE SET digest=excluded.digest,version=excluded.version,core_node=excluded.core_node,metadata=excluded.metadata,checked_at=excluded.checked_at,changed_at=excluded.changed_at,last_error=NULL,stale_after=excluded.stale_after",params![s.project_id,s.digest,version,s.core_node,metadata.to_string(),s.observed_at,s.observed_at,s.stale_after_seconds])?;
        // Immutable source observations are separate from execution checkpoints. Keep 20 bounded snapshots.
        tx.execute(
            "INSERT INTO map_local_history VALUES(?,?,?,?,?)",
            params![
                s.project_id,
                version,
                s.digest,
                s.observed_at,
                serde_json::to_string(s).unwrap()
            ],
        )?;
        tx.execute(
            "DELETE FROM map_local_history WHERE project_id=? AND version<=?",
            params![s.project_id, version - 20],
        )?;
        event(&tx, &s.project_id, None, "local_source_refresh", &s.digest)?;
        tx.commit()?;
        Ok(
            json!({"changed":true,"version":version,"digest":s.digest,"nodes":s.nodes.len(),"files":s.files.len()}),
        )
    }
    pub fn local_source_failure(&self, project: &str, message: &str) -> Result<()> {
        self.connect(false)?.execute(
            "UPDATE map_local_sources SET checked_at=unixepoch(),last_error=? WHERE project_id=?",
            params![message.chars().take(300).collect::<String>(), project],
        )?;
        Ok(())
    }
}
pub fn status(tx: &Transaction, project: &str) -> Result<Value> {
    let row:Option<(String,i64,String,i64,i64,Option<String>,i64)>=tx.query_row("SELECT digest,version,metadata,checked_at,changed_at,last_error,stale_after FROM map_local_sources WHERE project_id=?",[project],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional()?;
    let Some((digest, version, metadata, checked, changed, error, stale)) = row else {
        return Ok(json!({"available":false}));
    };
    let now: i64 = tx.query_row("SELECT unixepoch()", [], |r| r.get(0))?;
    let mut data: Value =
        serde_json::from_str(&metadata).map_err(|_| Error::Invalid("source metadata"))?;
    if let Some(gaps) = data["metadata"]["gaps"].as_array() {
        let total = data["metadata"]["gap_count_total"]
            .as_u64()
            .unwrap_or(gaps.len() as u64);
        let preview = gaps.iter().take(15).cloned().collect::<Vec<_>>();
        data["metadata"]["gaps"] = json!(preview);
        data["metadata"]["gap_count"] = json!(total);
    }
    Ok(
        json!({"available":true,"project_id":project,"version":version,"digest":digest,"checked_at":checked,"changed_at":changed,"freshness":if error.is_some(){"failed"}else if now-checked>stale{"stale"}else{"fresh"},"last_error":error,"source":data,"version_state":"local_unversioned_snapshot","acceptance":"not_imported"}),
    )
}
pub fn node(tx: &Transaction, id: &str) -> Result<Option<Value>> {
    let encoded: Option<String> = tx
        .query_row(
            "SELECT details FROM map_local_nodes WHERE node_id=?",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    encoded
        .map(|e| serde_json::from_str(&e).map_err(|_| Error::Invalid("source node")))
        .transpose()
}
fn cursor(tx: &Transaction, project: &str, after: &str) -> Result<(i64, String)> {
    let version: Option<i64> = tx
        .query_row(
            "SELECT version FROM map_local_sources WHERE project_id=?",
            [project],
            |r| r.get(0),
        )
        .optional()?;
    let Some(version) = version else {
        return Err(Error::Missing);
    };
    if after.is_empty() {
        return Ok((version, String::new()));
    }
    let (v, key) = after
        .split_once(':')
        .ok_or(Error::Invalid("source cursor"))?;
    if v.parse::<i64>().ok() != Some(version) {
        return Err(Error::Conflict("source snapshot changed"));
    }
    Ok((version, key.into()))
}
fn page(mut items: Vec<Value>, limit: usize, version: i64, key: &str) -> Value {
    let next = if items.len() > limit {
        items.truncate(limit);
        items
            .last()
            .and_then(|v| v[key].as_str())
            .map(|k| format!("{version}:{k}"))
    } else {
        None
    };
    json!({"items":items,"next":next,"source_version":version})
}
pub fn files(tx: &Transaction, id: &str, after: &str, limit: usize) -> Result<Value> {
    let project: Option<String> = tx
        .query_row(
            "SELECT project_id FROM map_local_nodes WHERE node_id=?",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(project) = project else {
        return Err(Error::Missing);
    };
    let (version, after) = cursor(tx, &project, after)?;
    let mut q=tx.prepare("SELECT f.metadata,l.category FROM map_local_links l JOIN map_local_files f ON f.project_id=l.project_id AND f.path=l.path WHERE l.node_id=? AND l.path>? GROUP BY l.path ORDER BY l.path LIMIT ?")?;
    let rows = q
        .query_map(params![id, after, (limit + 1) as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let items = rows
        .into_iter()
        .map(|(s, c)| {
            let mut v: Value = serde_json::from_str(&s).unwrap();
            v.as_object_mut().unwrap().remove("preview");
            v["category"] = json!(c);
            v
        })
        .collect();
    Ok(page(items, limit, version, "path"))
}
pub fn file(tx: &Transaction, project: &str, path: &str) -> Result<Value> {
    crate::resources::valid_path(path)?;
    let data: Option<String> = tx
        .query_row(
            "SELECT metadata FROM map_local_files WHERE project_id=? AND path=?",
            params![project, path],
            |r| r.get(0),
        )
        .optional()?;
    serde_json::from_str(&data.ok_or(Error::Missing)?).map_err(|_| Error::Invalid("source file"))
}
pub fn tree(
    tx: &Transaction,
    project: &str,
    path: &str,
    after: &str,
    limit: usize,
) -> Result<Value> {
    crate::resources::valid_path(path)?;
    let (version, after) = cursor(tx, project, after)?;
    if !path.is_empty() && file(tx, project, path)?["kind"] != "directory" {
        return Err(Error::Invalid("not indexed directory"));
    }
    let prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{path}/")
    };
    let mut q = tx.prepare(
        "SELECT path,metadata FROM map_local_files WHERE project_id=? AND path>? ORDER BY path",
    )?;
    let mut items = Vec::new();
    for row in q.query_map(params![project, after], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (p, m) = row?;
        if let Some(rest) = p.strip_prefix(&prefix) {
            if !rest.is_empty() && !rest.contains('/') {
                let mut v: Value = serde_json::from_str(&m).unwrap();
                v.as_object_mut().unwrap().remove("preview");
                items.push(v);
                if items.len() > limit {
                    break;
                }
            }
        }
    }
    Ok(page(items, limit, version, "path"))
}
pub fn lookup(
    tx: &Transaction,
    project: &str,
    path: &str,
    after: &str,
    limit: usize,
) -> Result<Value> {
    crate::resources::valid_path(path)?;
    let (version, after) = cursor(tx, project, after)?;
    let mut q=tx.prepare("SELECT DISTINCT l.node_id,n.title FROM map_local_links l JOIN map_dev_nodes n ON n.id=l.node_id JOIN map_local_files f ON f.project_id=l.project_id AND f.path=l.path WHERE l.project_id=? AND (l.path=? OR (json_extract(f.metadata,'$.kind')='directory' AND substr(?,1,length(l.path)+1)=l.path||'/')) AND l.node_id>? ORDER BY l.node_id LIMIT ?")?;
    let items = q
        .query_map(
            params![project, path, path, after, (limit + 1) as i64],
            |r| Ok(json!({"node_id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?})),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(items, limit, version, "node_id"))
}
pub fn history(
    tx: &Transaction,
    project: &str,
    node_id: Option<&str>,
    after: i64,
    limit: usize,
) -> Result<Value> {
    let mut q=tx.prepare("SELECT version,digest,observed_at,snapshot FROM map_local_history WHERE project_id=? AND (?=0 OR version<?) ORDER BY version DESC LIMIT ?")?;
    let mut items = Vec::new();
    for row in q.query_map(params![project, after, after, (limit + 1) as i64], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
        ))
    })? {
        let (version, digest, time, encoded) = row?;
        let snapshot: Snapshot =
            serde_json::from_str(&encoded).map_err(|_| Error::Invalid("source history"))?;
        let detail = node_id.and_then(|id| snapshot.nodes.into_iter().find(|n| n.id == id));
        items.push(json!({"version":version,"digest":digest,"observed_at":time,"node":detail,"source_checkpoint":snapshot.metadata.get("source_checkpoint"),"version_state":"local_unversioned_snapshot"}));
    }
    let next = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|v| v["version"].clone())
    } else {
        None
    };
    Ok(json!({"items":items,"next":next}))
}
