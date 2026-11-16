// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
//! Immutable Git observations: renames and unmerged commits never rewrite historical links.
use crate::store::{artifact_tx, event, ArtifactInput, Error, MapStore, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{path::Path, process::Command};
pub(crate) fn immutable_git_command() -> Command {
    let mut cmd = Command::new("git");
    cmd.env("GIT_NO_REPLACE_OBJECTS", "1");
    cmd
}
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = immutable_git_command()
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|_| Error::Invalid("Git unavailable"))?;
    if !out.status.success() {
        return Err(Error::Invalid("Git object or repository missing"));
    }
    Ok(out.stdout)
}
pub(crate) fn oid(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}
impl MapStore {
    pub fn register_repository(&self, key: &str, path: &Path, source: &str) -> Result<()> {
        if key.is_empty() || source.is_empty() {
            return Err(Error::Invalid("repository identity"));
        }
        let root = path
            .canonicalize()
            .map_err(|_| Error::Invalid("repository path"))?;
        git(&root, &["rev-parse", "--git-dir"])?;
        let top = git(&root, &["rev-parse", "--show-toplevel"])?;
        let top = std::str::from_utf8(&top)
            .map_err(|_| Error::Invalid("repository path encoding"))?
            .trim_end();
        if Path::new(top)
            .canonicalize()
            .map_err(|_| Error::Invalid("repository root"))?
            != root
        {
            return Err(Error::Invalid(
                "register the repository root, not a subdirectory",
            ));
        }
        self.connect(false)?.execute("INSERT INTO map_repositories VALUES(?,?,?) ON CONFLICT(repo_key) DO UPDATE SET path=excluded.path,source_ref=excluded.source_ref",params![key,root.to_string_lossy(),source])?;
        Ok(())
    }
    pub fn scan_git(&self, key: &str, from: &str, to: &str) -> Result<usize> {
        if !oid(from) || !oid(to) {
            return Err(Error::Invalid("full immutable Git revisions required"));
        }
        let mut db = self.connect(false)?;
        let root: String = db.query_row(
            "SELECT path FROM map_repositories WHERE repo_key=?",
            [key],
            |r| r.get(0),
        )?;
        let bytes = git(
            Path::new(&root),
            &[
                "diff",
                "--name-status",
                "-z",
                "--find-renames",
                from,
                to,
                "--",
            ],
        )?;
        let fields = bytes
            .split(|b| *b == 0)
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        let mut changes = Vec::new();
        let mut i = 0;
        while i < fields.len() {
            let status = String::from_utf8(fields[i].to_vec())
                .map_err(|_| Error::Invalid("Git status encoding"))?;
            i += 1;
            let old = if status.starts_with('R') || status.starts_with('C') {
                let p = fields.get(i).ok_or(Error::Invalid("Git rename"))?;
                i += 1;
                Some(
                    String::from_utf8(p.to_vec())
                        .map_err(|_| Error::Invalid("non-UTF8 Git path"))?,
                )
            } else {
                None
            };
            let p = fields.get(i).ok_or(Error::Invalid("Git path"))?;
            i += 1;
            let path =
                String::from_utf8(p.to_vec()).map_err(|_| Error::Invalid("non-UTF8 Git path"))?;
            changes.push((status, old, path));
        }
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let previous_count:Option<i64>=tx.query_row("SELECT count FROM map_git_scans WHERE repo_key=? AND from_revision=? AND to_revision=?",params![key,from,to],|r|r.get(0)).optional()?;
        if let Some(count) = previous_count {
            return usize::try_from(count).map_err(|_| Error::Invalid("negative git count"));
        }
        for (i, (status, old, path)) in changes.iter().enumerate() {
            tx.execute(
                "INSERT OR IGNORE INTO map_git_observations VALUES(?,?,?,?,?,?,?)",
                params![key, from, to, i as i64, status, old, path],
            )?;
        }
        let mut affected = std::collections::BTreeSet::new();
        for (status, old, path) in &changes {
            let previous = old.as_deref().unwrap_or(path);
            let mut q=tx.prepare("SELECT DISTINCT n.id,n.project_id FROM map_artifacts a JOIN map_local_node_artifacts l ON l.artifact_id=a.id JOIN map_dev_nodes n ON n.id=l.node_id WHERE a.repo_key=? AND a.revision=? AND (a.path=? OR (a.kind='directory' AND substr(?,1,length(a.path)+1)=a.path||'/'))")?;
            let nodes = q
                .query_map(params![key, from, previous, previous], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (node, project) in nodes {
                // Deletions affect the previous owner but have no new Git object.
                affected.insert((project, node.clone()));
                if status.starts_with('D') {
                    continue;
                }
                artifact_tx(
                    &tx,
                    &ArtifactInput {
                        node_id: node.clone(),
                        repo_key: key.into(),
                        revision: to.into(),
                        path: path.clone(),
                        kind: "file".into(),
                        provenance: "observed".into(),
                        verification: "verified".into(),
                        content_digest: None,
                    },
                )?;
            }
        }
        let already_scanned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM map_git_scans WHERE repo_key=? AND from_revision=? AND to_revision=?)",params![key,from,to],|r|r.get(0))?;
        if !already_scanned {
            for (project, node) in affected {
                event(&tx, &project, Some(&node), "git_observation", to)?;
            }
        }
        tx.execute(
            "INSERT OR IGNORE INTO map_git_scans VALUES(?,?,?,?)",
            params![key, from, to, changes.len() as i64],
        )?;
        tx.commit()?;
        Ok(changes.len())
    }
    pub fn git_changes(
        &self,
        key: &str,
        from: &str,
        to: &str,
        after: i64,
        n: usize,
    ) -> Result<Value> {
        self.read(|tx|{
  let n=crate::store::limit(Some(n));let mut q=tx.prepare("SELECT ordinal,status,old_path,new_path FROM map_git_observations WHERE repo_key=? AND from_revision=? AND to_revision=? AND ordinal>? ORDER BY ordinal LIMIT ?")?;
  let mut items=q.query_map(params![key,from,to,after,(n+1) as i64],|r|Ok(json!({"ordinal":r.get::<_,i64>(0)?,"status":r.get::<_,String>(1)?,"old_path":r.get::<_,Option<String>>(2)?,"path":r.get::<_,String>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
  let more=items.len()>n;items.truncate(n);let next=if more{items.last().map(|v|v["ordinal"].clone())}else{None};Ok(json!({"items":items,"next":next,"has_more":more,"historical_links_rewritten":false}))
 })
    }
}
