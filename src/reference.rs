// Copyright (c) 2026 EvanBo. Explicit operator grants pin one checkpoint summary.
use crate::store::{Error, MapStore, Result};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    recipient: String,
    checkpoint_id: i64,
    decision_ref: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revoke {
    reference_id: String,
    decision_ref: String,
}
impl MapStore {
    pub fn grant_reference(&self, g: &Grant) -> Result<String> {
        if g.decision_ref.trim().is_empty() || g.decision_ref.len() > 512 {
            return Err(Error::Invalid("explicit grant reference required"));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        crate::store::require_current_schema(&tx)?;
        let id = uuid::Uuid::new_v4().to_string();
        tx.execute("INSERT INTO map_checkpoint_references(id,recipient,checkpoint_id,decision_ref)
            SELECT ?1,r.id,c.id,?4 FROM map_dev_nodes r,map_checkpoint_index c JOIN map_dev_nodes src ON src.id=c.node_id
            WHERE r.id=?2 AND c.id=?3 AND src.project_id<>r.project_id
            ON CONFLICT(recipient,checkpoint_id,decision_ref) DO NOTHING",params![id,g.recipient,g.checkpoint_id,g.decision_ref])?;
        let active:Option<String>=tx.query_row("SELECT id FROM map_checkpoint_references WHERE recipient=? AND checkpoint_id=? AND decision_ref=? AND revoked_by IS NULL",
            params![g.recipient,g.checkpoint_id,g.decision_ref],|r|r.get(0)).optional()?;
        tx.commit()?;
        active.ok_or(Error::Conflict(
            "no valid grant; verify target or provide a new authorization",
        ))
    }
    pub fn revoke_reference(&self, q: &Revoke) -> Result<()> {
        if q.decision_ref.trim().is_empty() || q.decision_ref.len() > 512 {
            return Err(Error::Invalid("revocation reference required"));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        crate::store::require_current_schema(&tx)?;
        if tx.execute("UPDATE map_checkpoint_references SET revoked_by=? WHERE id=? AND (revoked_by IS NULL OR revoked_by=?)",params![q.decision_ref,q.reference_id,q.decision_ref])?!=1{return Err(Error::Missing);}
        tx.commit()?;
        Ok(())
    }
}
pub(crate) fn read(tx: &Transaction, recipient: &str, id: &str) -> Result<Value> {
    tx.query_row("SELECT c.id,c.summary,c.digest FROM map_checkpoint_references g JOIN map_checkpoint_index c ON c.id=g.checkpoint_id
        WHERE g.id=? AND g.recipient=? AND g.revoked_by IS NULL",params![id,recipient],
        |r|Ok(json!({"checkpoint_id":r.get::<_,i64>(0)?,"summary":r.get::<_,String>(1)?,"digest":r.get::<_,String>(2)?}))).optional()?.ok_or(Error::Missing)
}
