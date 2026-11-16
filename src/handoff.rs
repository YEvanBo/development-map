// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
use crate::store::{event, Error, MapStore, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
pub struct Handoff {
    pub node_id: String,
    pub expected_owner: Option<String>,
    pub expected_generation: i64,
    pub new_owner: String,
    pub source_ref: String,
}
impl MapStore {
    pub fn handoff(&self, h: &Handoff, token: Option<&str>) -> Result<i64> {
        if h.new_owner.is_empty()
            || h.source_ref.is_empty()
            || h.expected_generation < 0
            || h.expected_generation == i64::MAX
        {
            return Err(Error::Invalid("handoff"));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let project: String = tx.query_row(
            "SELECT project_id FROM map_dev_nodes WHERE id=?",
            [&h.node_id],
            |r| r.get(0),
        )?;
        if let Some(t) = token {
            crate::identity::authorize(&tx, t, &project, Some(&h.node_id), "manager")?;
        }
        let generation = h.expected_generation + 1;
        let changed=tx.execute("UPDATE map_dev_nodes SET owner=?,generation=?,updated_at=unixepoch() WHERE id=? AND owner IS ? AND generation=?",params![h.new_owner,generation,h.node_id,h.expected_owner,h.expected_generation])?;
        if changed != 1 {
            return Err(Error::Conflict("handoff assignment changed"));
        }
        event(&tx, &project, Some(&h.node_id), "handoff", &h.source_ref)?;
        tx.commit()?;
        Ok(generation)
    }
}
