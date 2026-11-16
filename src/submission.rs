// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
//! A checkpoint, its exact artifact set, event and rollups commit together.
use crate::store::{artifact_tx, event, ArtifactInput, CheckpointInput, Error, MapStore, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub(crate) const STALE_BASE: &str = "stale checkpoint base: preserve pending work, resume and reconcile before submitting";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submission {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_checkpoint_rowid: Option<i64>,
    pub submission_key: String,
    pub checkpoint: CheckpointInput,
    pub artifacts: Vec<ArtifactInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<crate::resources::Manifest>,
}
#[derive(Serialize)]
pub struct Receipt {
    pub created: bool,
    pub checkpoint_rowid: i64,
    pub event_id: i64,
    pub payload_digest: String,
}
impl MapStore {
    pub fn submit(&self, input: &Submission) -> Result<Receipt> {
        self.submit_bound(input, None)
    }
    pub fn submit_bound(&self, input: &Submission, token: Option<&str>) -> Result<Receipt> {
        let c = &input.checkpoint;
        if input.submission_key.is_empty()
            || input.submission_key.len() > 256
            || input.artifacts.len() > 100
            || c.version < 1
            || c.digest.len() < 16
            || c.summary.len() > 2048
            || c.source_ref.is_empty()
        {
            return Err(Error::Invalid("submission fields"));
        }
        let payload = if let Some(base) = input.base_checkpoint_rowid {
            serde_json::to_vec(&(c, &input.artifacts, &input.resources, base))
        } else if let Some(m) = &input.resources {
            serde_json::to_vec(&(c, &input.artifacts, m))
        } else {
            serde_json::to_vec(&(c, &input.artifacts))
        }
        .map_err(|_| Error::Invalid("submission JSON"))?;
        if payload.len() > 60000 {
            return Err(Error::Invalid("submission too large"));
        }
        let digest = format!("{:x}", Sha256::digest(&payload));
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        crate::store::require_current_schema(&tx)?;
        let state: Option<(String, Option<String>, i64)> = tx
            .query_row(
                "SELECT project_id,owner,generation FROM map_dev_nodes WHERE id=?",
                [&c.node_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((project, owner, generation)) = state else {
            return Err(Error::Missing);
        };
        let local: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM map_local_nodes WHERE node_id=?)",
            [&c.node_id],
            |r| r.get(0),
        )?;
        if local {
            return Err(Error::Conflict(
                "local source nodes are read-only; use original checkpoint contract",
            ));
        }
        let mut identity = None;
        if let Some(token) = token {
            let b = crate::identity::authorize(&tx, token, &project, Some(&c.node_id), "worker")?;
            if b.owner != c.owner || b.generation != Some(c.generation) {
                return Err(Error::Invalid("checkpoint identity mismatch"));
            }
            identity = Some(b);
        }
        if owner.as_deref() != Some(c.owner.as_str()) || generation != c.generation {
            return Err(Error::Conflict("owner or generation"));
        }
        let old:Option<(String,i64,i64)>=tx.query_row("SELECT payload_digest,checkpoint_rowid,event_id FROM map_submissions WHERE node_id=? AND submission_key=?",params![c.node_id,input.submission_key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((d, checkpoint_rowid, event_id)) = old {
            if d != digest {
                return Err(Error::Conflict("submission key payload changed"));
            }
            return Ok(Receipt {
                created: false,
                checkpoint_rowid,
                event_id,
                payload_digest: d,
            });
        }
        if token.is_some() || input.base_checkpoint_rowid.is_some() {
            let head: i64 = tx.query_row(
                "SELECT COALESCE((SELECT id FROM map_checkpoint_index WHERE node_id=? ORDER BY generation DESC,version DESC LIMIT 1),0)",
                [&c.node_id], |r| r.get(0),
            )?;
            if input.base_checkpoint_rowid.unwrap_or(0) != head {
                return Err(Error::Conflict(STALE_BASE));
            }
        }
        let latest: Option<i64> = tx.query_row(
            "SELECT MAX(version) FROM map_checkpoint_index WHERE node_id=? AND generation=?",
            params![c.node_id, c.generation],
            |r| r.get(0),
        )?;
        if latest.is_some_and(|v| c.version <= v) {
            return Err(Error::Conflict(
                "checkpoint version already used or regressed",
            ));
        }
        let arch: String = tx.query_row(
            "SELECT version FROM map_architectures WHERE project_id=?",
            [&project],
            |r| r.get(0),
        )?;
        tx.execute("INSERT INTO map_checkpoint_index(node_id,owner,generation,version,digest,summary,source_ref,git_revision,arch_version,created_at) VALUES(?,?,?,?,?,?,?,?,?,unixepoch())",params![c.node_id,c.owner,c.generation,c.version,c.digest,c.summary,c.source_ref,c.git_revision,arch])?;
        let checkpoint_rowid = tx.last_insert_rowid();
        if let Some(b) = identity {
            tx.execute("INSERT INTO map_submission_identity(checkpoint_id,subject,task_id,owner,source_ref) VALUES(?,?,?,?,?)",params![checkpoint_rowid,b.subject,b.task_id,b.owner,b.source_ref])?;
        }
        for a in &input.artifacts {
            if a.node_id != c.node_id || a.verification == "verified" {
                return Err(Error::Invalid("submission artifact node or verification"));
            }
            let artifact = artifact_tx(&tx, a)?;
            tx.execute(
                "INSERT INTO map_checkpoint_artifacts VALUES(?,?,?)",
                params![checkpoint_rowid, artifact, a.provenance],
            )?;
        }
        if let Some(m) = &input.resources {
            crate::resources::save(&tx, &project, c, checkpoint_rowid, m)?;
        }
        event(
            &tx,
            &project,
            Some(&c.node_id),
            "submission",
            &input.submission_key,
        )?;
        let event_id = tx.query_row("SELECT MAX(id) FROM map_events", [], |r| r.get(0))?;
        tx.execute(
            "INSERT INTO map_submissions VALUES(?,?,?,?,?)",
            params![
                input.submission_key,
                c.node_id,
                digest,
                checkpoint_rowid,
                event_id
            ],
        )?;
        tx.commit()?;
        Ok(Receipt {
            created: true,
            checkpoint_rowid,
            event_id,
            payload_digest: digest,
        })
    }
}
