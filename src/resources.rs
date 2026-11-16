// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
//! Versioned resource manifests. Git objects are read without checkout or executing repository code.
use crate::store::{CheckpointInput, Error, MapStore, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::Path, process::Command};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub key: String,
    pub category: String,
    pub repo_key: Option<String>,
    pub revision: Option<String>,
    pub path: Option<String>,
    pub ownership: String,
    pub required: bool,
    pub not_applicable: bool,
    pub reason: String,
    pub database: Option<DatabaseRef>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseRef {
    pub schema: String,
    pub entity: String,
    pub record: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub architecture_revision: String,
    pub hierarchy_exception: String,
    pub entries: Vec<Resource>,
}
fn invalid(s: &'static str) -> Error {
    Error::Invalid(s)
}
pub fn valid_path(path: &str) -> Result<()> {
    if path.split('/').count() > 64
        || path.len() > 2048
        || path.starts_with('/')
        || path.contains(['\\', '\0', ':'])
        || (!path.is_empty()
            && path
                .split('/')
                .any(|x| x.is_empty() || x == "." || x == ".."))
    {
        return Err(invalid(
            "resource path must be a normalized repository relative path",
        ));
    }
    Ok(())
}
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .output()
        .map_err(|_| invalid("Git unavailable"))?;
    if !out.status.success() {
        return Err(invalid("Git object unavailable"));
    }
    Ok(out.stdout)
}
fn root(tx: &Transaction, project: &str, repo: &str) -> Result<String> {
    tx.query_row("SELECT r.path FROM map_repositories r JOIN map_project_repositories p USING(repo_key) WHERE p.project_id=? AND r.repo_key=?",params![project,repo],|r|r.get(0)).optional()?.ok_or(invalid("repository is not registered to this project"))
}
// Reject all symlinks (including internal ones) and submodule traversal. Never follow repository paths.
fn safe_local(root: &Path, path: &str) -> Result<()> {
    let mut p = root.to_path_buf();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        p.push(part);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(invalid("symbolic links are not resource paths"))
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(invalid("resource path cannot be checked")),
        }
    }
    Ok(())
}
fn tree_entry(root: &Path, rev: &str, path: &str) -> Result<Option<(String, String)>> {
    if path.is_empty() {
        return Ok(Some(("040000".into(), "tree".into())));
    }
    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
    let bytes = git(root, &["ls-tree", "-z", &format!("{rev}:{parent}")])?;
    for entry in bytes.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let s = std::str::from_utf8(entry).map_err(|_| invalid("non UTF-8 repository path"))?;
        let (meta, p) = s.split_once('\t').ok_or(invalid("Git tree record"))?;
        if p == name {
            let mut m = meta.split_whitespace();
            return Ok(Some((
                m.next().unwrap_or("").into(),
                m.next().unwrap_or("").into(),
            )));
        }
    }
    Ok(None)
}
fn safe_git(root: &Path, rev: &str, path: &str) -> Result<()> {
    let mut prefix = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        match tree_entry(root, rev, &prefix) {
            Ok(Some((mode, _))) if mode == "120000" || mode == "160000" => {
                return Err(invalid("Git symlink or submodule paths are not supported"))
            }
            Ok(None) | Err(_) => break,
            _ => {}
        }
    }
    Ok(())
}
fn status(
    tx: &Transaction,
    project: &str,
    cp_revision: Option<&str>,
    r: &Resource,
) -> Result<&'static str> {
    if r.not_applicable {
        return Ok("not_applicable");
    }
    let (Some(repo), Some(rev), Some(path)) = (&r.repo_key, &r.revision, &r.path) else {
        return Ok("missing");
    };
    let dir = root(tx, project, repo)?;
    safe_local(Path::new(&dir), path)?;
    if Some(rev.as_str()) != cp_revision {
        return Ok("version_mismatch");
    }
    if git(
        Path::new(&dir),
        &["cat-file", "-e", &format!("{rev}^{{commit}}")],
    )
    .is_err()
    {
        return Ok("pending");
    }
    safe_git(Path::new(&dir), rev, path)?;
    if r.category == "database" {
        let Some(db) = &r.database else {
            return Ok("missing");
        };
        // Only the local map record is introspectable here. External business DB declarations remain pending.
        if db.schema != "main" || db.entity != "map_dev_nodes" {
            return Ok("pending");
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM map_dev_nodes WHERE id=? AND project_id=?)",
            params![db.record, project],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok("missing");
        }
    }
    match tree_entry(Path::new(&dir), rev, path) {
        Ok(Some((_, kind)))
            if (r.category == "directory" && kind == "tree")
                || (r.category != "directory" && kind == "blob") =>
        {
            Ok("verified")
        }
        _ => Ok("missing"),
    }
}
impl MapStore {
    /// Trusted local registration; remote workers cannot grant themselves filesystem access.
    pub fn register_project_repository(&self, project: &str, key: &str) -> Result<()> {
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO map_project_repositories(project_id,repo_key) VALUES(?,?)",
            params![project, key],
        )?;
        if inserted > 0 {
            crate::store::event(
                &tx,
                project,
                None,
                "resource_policy",
                "project repository linked",
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}
pub(crate) fn save(
    tx: &Transaction,
    project: &str,
    c: &CheckpointInput,
    cp: i64,
    m: &Manifest,
) -> Result<()> {
    let adopted: String = tx.query_row(
        "SELECT version FROM map_architectures WHERE project_id=?",
        [project],
        |r| r.get(0),
    )?;
    if adopted != m.architecture_revision {
        return Err(Error::Conflict("resource architecture revision mismatch"));
    }
    if m.entries.len() > 100
        || m.hierarchy_exception.len() > 2048
        || serde_json::to_vec(m)
            .map_err(|_| invalid("manifest JSON"))?
            .len()
            > 16000
    {
        return Err(invalid("resource manifest bounds"));
    }
    let mut keys = std::collections::HashSet::new();
    for r in &m.entries {
        if r.key.is_empty()
            || r.key.len() > 128
            || !keys.insert(&r.key)
            || r.reason.len() > 2048
            || !matches!(
                r.category.as_str(),
                "directory" | "code" | "document" | "database" | "checkpoint" | "evidence"
            )
            || !matches!(r.ownership.as_str(), "primary" | "shared")
        {
            return Err(invalid("resource identity category or ownership"));
        }
        if r.not_applicable {
            if r.reason.trim().is_empty()
                || r.path.is_some()
                || r.repo_key.is_some()
                || r.revision.is_some()
                || r.database.is_some()
            {
                return Err(invalid("not applicable requires reason and no location"));
            }
        } else {
            if let Some(path) = &r.path {
                valid_path(path)?;
            }
            if let Some(rev) = &r.revision {
                if !crate::git_index::oid(rev) {
                    return Err(invalid("resource requires immutable Git revision"));
                }
            }
            if let Some(repo) = &r.repo_key {
                root(tx, project, repo)?;
            }
            if r.category == "database"
                && r.database.as_ref().is_none_or(|d| {
                    d.schema.is_empty() || d.entity.is_empty() || d.record.is_empty()
                })
            {
                return Err(invalid(
                    "business database schema entity and record required",
                ));
            }
            if r.database
                .as_ref()
                .is_some_and(|d| d.entity == "map_dev_nodes" && d.record != c.node_id)
            {
                return Err(invalid("database record belongs to another node"));
            }
            status(tx, project, c.git_revision.as_deref(), r)?;
        }
    }
    if m.entries
        .iter()
        .filter(|r| r.category == "directory")
        .count()
        != 1
    {
        return Err(invalid(
            "exactly one main directory or explicit not applicable required",
        ));
    }
    let main = m
        .entries
        .iter()
        .find(|r| r.category == "directory")
        .unwrap();
    if !main.not_applicable && main.ownership != "primary" {
        return Err(invalid("main directory must be primary"));
    }
    // Enforce containment against the latest parent manifest; absence stays visibly pending.
    let parent: Option<String> = tx.query_row(
        "SELECT parent_id FROM map_dev_nodes WHERE id=?",
        [&c.node_id],
        |r| r.get(0),
    )?;
    let mut hierarchy = "verified";
    if let Some(parent) = parent {
        let parent_manifest:Option<String>=tx.query_row("SELECT manifest FROM map_resource_sets s JOIN map_checkpoint_index c ON c.id=s.checkpoint_id WHERE c.node_id=? ORDER BY c.generation DESC,c.version DESC LIMIT 1",[parent],|r|r.get(0)).optional()?;
        if !main.not_applicable {
            if let Some(encoded) = parent_manifest {
                let p: Manifest =
                    serde_json::from_str(&encoded).map_err(|_| invalid("stored manifest"))?;
                let dir = p.entries.iter().find(|r| r.category == "directory");
                let fits = dir.is_some_and(|d| {
                    d.not_applicable
                        || (d.repo_key == main.repo_key
                            && d.revision == main.revision
                            && d.path
                                .as_ref()
                                .zip(main.path.as_ref())
                                .is_some_and(|(a, b)| {
                                    a.is_empty() || a == b || b.starts_with(&format!("{a}/"))
                                }))
                });
                if !fits {
                    if m.hierarchy_exception.trim().is_empty() {
                        return Err(Error::Conflict("child directory is outside parent directory; explicit exception required"));
                    }
                    hierarchy = "exception";
                }
            } else {
                hierarchy = "pending";
            }
        }
    }
    for r in &m.entries {
        if r.ownership == "primary" && !r.not_applicable && r.category != "directory" {
            if let Some(path) = &r.path {
                if let Some(dir) = &main.path {
                    if (r.repo_key != main.repo_key
                        || !(dir.is_empty() || path == dir || path.starts_with(&format!("{dir}/"))))
                        && m.hierarchy_exception.trim().is_empty()
                    {
                        return Err(Error::Conflict("primary resource outside main directory"));
                    }
                }
            }
            let clash:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM map_resource_entries e JOIN map_checkpoint_index c ON c.id=e.checkpoint_id WHERE e.repo_key=? AND e.revision=? AND e.path=? AND e.ownership='primary' AND c.node_id<>? AND c.id=(SELECT id FROM map_checkpoint_index WHERE node_id=c.node_id ORDER BY generation DESC,version DESC LIMIT 1))",params![r.repo_key,r.revision,r.path,c.node_id],|row|row.get(0))?;
            if clash {
                return Err(Error::Conflict(
                    "file already has primary owner; use shared reference",
                ));
            }
        }
    }
    tx.execute(
        "INSERT INTO map_resource_sets VALUES(?,?,?)",
        params![
            cp,
            serde_json::to_string(m).map_err(|_| invalid("manifest"))?,
            hierarchy
        ],
    )?;
    for r in &m.entries {
        tx.execute(
            "INSERT INTO map_resource_entries VALUES(?,?,?,?,?,?,?,?)",
            params![
                cp,
                r.key,
                r.category,
                r.repo_key,
                r.revision,
                r.path,
                r.ownership,
                serde_json::to_string(r).unwrap()
            ],
        )?;
        if !r.not_applicable
            && r.repo_key.is_some()
            && r.revision.is_some()
            && r.path.as_ref().is_some_and(|p| !p.is_empty())
        {
            let a = crate::store::ArtifactInput {
                node_id: c.node_id.clone(),
                repo_key: r.repo_key.clone().unwrap(),
                revision: r.revision.clone().unwrap(),
                path: r.path.clone().unwrap(),
                kind: if r.category == "directory" {
                    "directory"
                } else if r.category == "document" {
                    "document"
                } else {
                    "file"
                }
                .into(),
                provenance: "declared".into(),
                verification: "unverified".into(),
                content_digest: None,
            };
            let id = crate::store::artifact_tx(tx, &a)?;
            tx.execute(
                "INSERT OR IGNORE INTO map_checkpoint_artifacts VALUES(?,?,?)",
                params![cp, id, "declared"],
            )?;
        }
    }
    Ok(())
}
pub fn manifest(tx: &Transaction, node: &str, checkpoint: Option<i64>) -> Result<Value> {
    let (project, parent, kind, owner, generation): (
        String,
        Option<String>,
        String,
        Option<String>,
        i64,
    ) = tx.query_row(
        "SELECT project_id,parent_id,kind,owner,generation FROM map_dev_nodes WHERE id=?",
        [node],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let cp:Option<(i64,Option<String>,String,i64,String,i64,String)>=tx.query_row("SELECT id,git_revision,arch_version,version,owner,generation,source_ref FROM map_checkpoint_index WHERE node_id=? AND (? IS NULL OR id=?) ORDER BY generation DESC,version DESC LIMIT 1",params![node,checkpoint,checkpoint],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional()?;
    if checkpoint.is_some() && cp.is_none() {
        return Err(Error::Missing);
    }
    let mut out = json!({"node_id":node,"project_id":project,"parent_id":parent,"kind":kind,"owner":owner,"generation":generation,"mapping_state":"missing","entries":[],"complete":false,"database_record":{"table":"map_dev_nodes","key":node,"url":format!("/v1/map/nodes/{}",percent_encoding::utf8_percent_encode(node,percent_encoding::NON_ALPHANUMERIC))}});
    if let Some((id, rev, arch, version, cp_owner, cp_gen, source)) = cp {
        out["checkpoint"] = json!({"id":id,"git_revision":rev,"architecture_revision":arch,"version":version,"owner":cp_owner,"generation":cp_gen,"source_ref":source});
        let saved: Option<(String, String)> = tx
            .query_row(
                "SELECT manifest,hierarchy_state FROM map_resource_sets WHERE checkpoint_id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((encoded, mut hierarchy)) = saved {
            let m: Manifest =
                serde_json::from_str(&encoded).map_err(|_| invalid("stored manifest"))?;
            // Re-evaluate parent mapping on every read/acceptance: a parent may have moved since submission.
            if let Some(parent) = &parent {
                let main = m
                    .entries
                    .iter()
                    .find(|r| r.category == "directory")
                    .ok_or(invalid("stored manifest missing directory"))?;
                if !main.not_applicable {
                    let encoded:Option<String> = tx.query_row("SELECT s.manifest FROM map_checkpoint_index c LEFT JOIN map_resource_sets s ON s.checkpoint_id=c.id WHERE c.node_id=? ORDER BY c.generation DESC,c.version DESC LIMIT 1",[parent],|r|r.get(0)).optional()?.flatten();
                    hierarchy = if let Some(encoded) = encoded {
                        let p: Manifest = serde_json::from_str(&encoded)
                            .map_err(|_| invalid("parent manifest"))?;
                        let fits = p
                            .entries
                            .iter()
                            .find(|r| r.category == "directory")
                            .is_some_and(|d| {
                                d.not_applicable
                                    || (d.repo_key == main.repo_key
                                        && d.revision == main.revision
                                        && d.path.as_ref().zip(main.path.as_ref()).is_some_and(
                                            |(a, b)| {
                                                a.is_empty()
                                                    || a == b
                                                    || b.starts_with(&format!("{a}/"))
                                            },
                                        ))
                            });
                        if fits {
                            "verified"
                        } else if !m.hierarchy_exception.trim().is_empty() {
                            "exception"
                        } else {
                            "pending"
                        }
                    } else {
                        "pending"
                    }
                    .into();
                }
            }
            let previous:Option<String>=tx.query_row("SELECT s.manifest FROM map_resource_sets s JOIN map_checkpoint_index c ON c.id=s.checkpoint_id WHERE c.node_id=? AND c.id<? ORDER BY c.generation DESC,c.version DESC LIMIT 1",params![node,id],|r|r.get(0)).optional()?;
            let old: Option<Manifest> = previous
                .map(|p| serde_json::from_str(&p))
                .transpose()
                .map_err(|_| invalid("previous manifest"))?;
            let mut changes = Vec::new();
            if let Some(old) = old {
                for entry in &m.entries {
                    match old.entries.iter().find(|r|r.key==entry.key) {
                        Some(before) if serde_json::to_value(before).unwrap()!=serde_json::to_value(entry).unwrap()=>changes.push(json!({"key":entry.key,"change":"updated","before":before,"after":entry})),
                        None=>changes.push(json!({"key":entry.key,"change":"added","after":entry})),
                        _=>{},
                    }
                }
                for entry in &old.entries {
                    if !m.entries.iter().any(|r| r.key == entry.key) {
                        changes.push(json!({"key":entry.key,"change":"removed","before":entry}));
                    }
                }
            }
            out["binding_changes"] = json!(changes);
            let mut entries = Vec::new();
            for r in &m.entries {
                let mut item = serde_json::to_value(r).unwrap();
                match status(tx, &project, rev.as_deref(), r) {
                    Ok(state) => item["status"] = json!(state),
                    Err(_) => {
                        item["status"] = json!("pending");
                        item["verification_note"]=json!("repository/path safety check failed; repair registration or path before acceptance");
                    }
                }
                entries.push(item);
            }
            let adopted: String = tx.query_row(
                "SELECT version FROM map_architectures WHERE project_id=?",
                [&project],
                |r| r.get(0),
            )?;
            let mut gaps = Vec::new();
            for category in ["directory", "code", "document", "database", "checkpoint"] {
                if !m
                    .entries
                    .iter()
                    .any(|r| r.category == category && r.required)
                {
                    gaps.push(format!("{category}: required declaration missing"));
                }
            }
            for e in &entries {
                if e["required"] == true
                    && e["status"] != "verified"
                    && e["status"] != "not_applicable"
                {
                    gaps.push(format!("{}: {}", e["key"], e["status"]));
                }
            }
            if hierarchy == "pending" {
                gaps.push("parent directory mapping missing".into());
            }
            if arch != adopted || cp_owner != owner.unwrap_or_default() || cp_gen != generation {
                gaps.push("checkpoint identity or architecture stale".into());
            }
            out["mapping_state"] = json!("registered");
            out["hierarchy_state"] = json!(hierarchy);
            out["hierarchy_exception"] = json!(m.hierarchy_exception);
            out["entries"] = json!(entries);
            out["complete"] = json!(gaps.is_empty());
            out["gaps"] = json!(gaps);
        }
    }
    Ok(out)
}
pub(crate) fn acceptance_gate(tx: &Transaction, node: &str) -> Result<()> {
    if manifest(tx, node, None)?["complete"] != true {
        return Err(Error::Conflict(
            "required resource bindings are incomplete or stale",
        ));
    }
    Ok(())
}

pub fn lookup(
    tx: &Transaction,
    project: &str,
    repo: &str,
    rev: &str,
    path: &str,
    after: &str,
    limit: usize,
) -> Result<Value> {
    valid_path(path)?;
    root(tx, project, repo)?;
    use sha2::{Digest, Sha256};
    let scope = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(project, repo, rev, path)).unwrap())
    );
    let event: i64 = tx.query_row(
        "SELECT COALESCE(MAX(id),0) FROM map_events WHERE project_id=?",
        [project],
        |r| r.get(0),
    )?;
    let after = if after.is_empty() {
        ""
    } else {
        let parts = after.splitn(3, ':').collect::<Vec<_>>();
        if parts.len() != 3 || parts[0].parse::<i64>().ok() != Some(event) || parts[1] != scope {
            return Err(Error::Conflict("reverse lookup snapshot or query changed"));
        }
        parts[2]
    };
    let mut q=tx.prepare("SELECT DISTINCT c.node_id FROM map_resource_entries e JOIN map_checkpoint_index c ON c.id=e.checkpoint_id JOIN map_dev_nodes n ON n.id=c.node_id WHERE n.project_id=? AND e.repo_key=? AND e.revision=? AND e.path=? AND c.node_id>? AND c.id=(SELECT id FROM map_checkpoint_index WHERE node_id=c.node_id ORDER BY generation DESC,version DESC LIMIT 1) ORDER BY c.node_id LIMIT ?")?;
    let mut nodes = q
        .query_map(
            params![project, repo, rev, path, after, (limit + 1) as i64],
            |r| r.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let next = if nodes.len() > limit {
        nodes.truncate(limit);
        nodes.last().map(|node| format!("{event}:{scope}:{node}"))
    } else {
        None
    };
    Ok(json!({"items":nodes,"next":next}))
}
pub fn directory(
    tx: &Transaction,
    node: &str,
    cp: i64,
    key: &str,
    path: &str,
    after: &str,
    limit: usize,
) -> Result<Value> {
    valid_path(path)?;
    let m = manifest(tx, node, Some(cp))?;
    let entries = m["entries"].as_array().ok_or(Error::Missing)?;
    let entry = entries
        .iter()
        .find(|e| e["key"] == key && e["category"] == "directory")
        .ok_or(Error::Missing)?;
    if entry["status"] != "verified" {
        return Err(Error::Conflict("directory is not verified"));
    }
    let base = entry["path"].as_str().ok_or(Error::Missing)?;
    if !base.is_empty() && path != base && !path.starts_with(&format!("{base}/")) {
        return Err(invalid("directory outside registered main directory"));
    }
    let root = root(
        tx,
        m["project_id"].as_str().unwrap(),
        entry["repo_key"].as_str().unwrap(),
    )?;
    let root = Path::new(&root);
    let rev = entry["revision"].as_str().unwrap();
    safe_local(root, path)?;
    safe_git(root, rev, path)?;
    let bytes = git(root, &["ls-tree", "-z", &format!("{rev}:{path}")])?;
    let mut items = Vec::new();
    for raw in bytes.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let raw = std::str::from_utf8(raw).map_err(|_| invalid("non UTF-8 path"))?;
        let (meta, name) = raw.split_once('\t').ok_or(invalid("Git tree record"))?;
        if name <= after {
            continue;
        }
        let parts = meta.split_whitespace().collect::<Vec<_>>();
        items.push(json!({"name":name,"path":if path.is_empty(){name.into()}else{format!("{path}/{name}")},"kind":parts[1],"supported":parts[0]!="120000"&&parts[0]!="160000"}));
    }
    items.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let next = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|v| v["name"].clone())
    } else {
        None
    };
    Ok(json!({"items":items,"next":next,"checkpoint_id":cp,"revision":rev,"path":path}))
}

/// Page a bounded manifest while pinning continuation to the same checkpoint snapshot.
pub fn page(
    tx: &Transaction,
    node: &str,
    checkpoint: Option<i64>,
    after: &str,
    limit: usize,
) -> Result<Value> {
    let mut value = manifest(tx, node, checkpoint)?;
    let cp = value["checkpoint"]["id"].as_i64().unwrap_or(0);
    let offset = if after.is_empty() {
        0
    } else {
        let (snapshot, offset) = after.split_once(':').ok_or(invalid("resource cursor"))?;
        if snapshot.parse::<i64>().ok() != Some(cp) {
            return Err(Error::Conflict("resource snapshot changed"));
        }
        offset
            .parse::<usize>()
            .map_err(|_| invalid("resource cursor"))?
    };
    let entries = value["entries"].as_array().unwrap();
    if offset > entries.len() {
        return Err(invalid("resource cursor offset"));
    }
    let end = (offset + limit.clamp(1, 20)).min(entries.len());
    let next = if end < entries.len() {
        Some(format!("{cp}:{end}"))
    } else {
        None
    };
    let selected = entries[offset..end].to_vec();
    let keys = selected
        .iter()
        .filter_map(|e| e["key"].as_str())
        .collect::<std::collections::HashSet<_>>();
    let changes = value["binding_changes"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|c| {
                    keys.contains(c["key"].as_str().unwrap_or(""))
                        || (offset == 0 && c["change"] == "removed")
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    value["binding_changes"] = json!(changes);
    if let Some(gaps) = value["gaps"].as_array() {
        let count = gaps.len();
        let shown = gaps.iter().take(10).cloned().collect::<Vec<_>>();
        value["gap_count"] = json!(count);
        value["gaps"] = json!(shown);
    }
    value["entries"] = json!(selected);
    value["next"] = json!(next);
    Ok(value)
}
