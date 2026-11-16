// Copyright (c) 2026 EvanBo. Operator-attested user turns; never inferred from worker prose.
use crate::store::{self, Error, MapStore, Result};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalConfirmation {
    pub project_id: String,
    pub node_id: String,
    pub generation: i64,
    pub previous: i64,
    pub goal: String,
    pub user_ref: String,
}

fn latest(tx: &Transaction, project: &str, node: &str) -> Result<Option<(i64, Value)>> {
    let event: Option<(i64,String)> = tx.query_row(
        "SELECT e.id,e.detail FROM map_events e LEFT JOIN map_goal_checkpoints g ON g.event_id=e.id WHERE e.project_id=? AND e.node_id=? AND ((e.kind='user_goal' AND g.event_id IS NOT NULL) OR e.kind='user_goal_revoked') ORDER BY e.id DESC LIMIT 1",
        params![project,node], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
    event
        .map(|(id, s)| {
            serde_json::from_str(&s)
                .map(|v| (id, v))
                .map_err(|_| Error::Invalid("invalid confirmation event"))
        })
        .transpose()
}

impl MapStore {
    pub fn confirm_goal(&self, q: &GoalConfirmation) -> Result<i64> {
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = Self::confirm_goal_tx(&tx, q)?;
        tx.commit()?;
        Ok(id)
    }
    pub(crate) fn confirm_goal_tx(tx: &Transaction, q: &GoalConfirmation) -> Result<i64> {
        if q.goal.trim().is_empty()
            || q.goal.len() > 2048
            || q.user_ref.trim().is_empty()
            || q.user_ref.len() > 512
        {
            return Err(Error::Invalid(
                "user goal and attributable turn reference required",
            ));
        }
        store::require_current_schema(tx)?;
        if !tx.query_row("SELECT EXISTS(SELECT 1 FROM map_dev_nodes WHERE project_id=? AND id=? AND generation=?)",
            params![q.project_id,q.node_id,q.generation],|r|r.get::<_,bool>(0))? {
            return Err(Error::Conflict("assignment mismatch"));
        }
        let prev = latest(tx, &q.project_id, &q.node_id)?;
        if let Some((id, v)) = &prev {
            if v["goal"] == q.goal && v["user_ref"] == q.user_ref {
                return Ok(*id);
            }
        }
        if prev.map(|(id, _)| id).unwrap_or(0) != q.previous {
            return Err(Error::Conflict(
                "confirmation changed; reread before correcting",
            ));
        }
        let reused:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM map_events e JOIN map_goal_checkpoints g ON g.event_id=e.id WHERE e.project_id=? AND e.node_id=? AND e.kind='user_goal' AND json_extract(e.detail,'$.user_ref')=?)",
            params![q.project_id,q.node_id,q.user_ref],|r|r.get(0))?;
        if reused {
            return Err(Error::Conflict(
                "user turn already attested; a new reply is required",
            ));
        }
        let head: i64 = tx.query_row(
            "SELECT coalesce(max(id),0) FROM map_checkpoint_index WHERE node_id=?",
            [&q.node_id],
            |r| r.get(0),
        )?;
        use sha2::{Digest, Sha256};
        let goal_hash = format!("{:x}", Sha256::digest(q.goal.as_bytes()));
        let evidence = json!({"previous":q.previous,"scope":"task_goal","goal_hash":goal_hash,"generation":q.generation,"goal":q.goal,"user_ref":q.user_ref,"base_checkpoint":head});
        store::event(
            tx,
            &q.project_id,
            Some(&q.node_id),
            "user_goal",
            &evidence.to_string(),
        )?;
        let id: i64 = tx.query_row(
            "SELECT max(id) FROM map_events WHERE project_id=? AND node_id=? AND kind='user_goal'",
            params![q.project_id, q.node_id],
            |r| r.get(0),
        )?;
        // The receipt refers to the attested text; it does not verify a product result.
        let owner: String = tx.query_row(
            "SELECT owner FROM map_dev_nodes WHERE id=?",
            [&q.node_id],
            |r| r.get(0),
        )?;
        let version:i64=tx.query_row("SELECT coalesce(max(version),0)+1 FROM map_checkpoint_index WHERE node_id=? AND generation=?",params![q.node_id,q.generation],|r|r.get(0))?;
        let receipt = Self::submit_tx(
            tx,
            &crate::submission::Submission {
                base_checkpoint_rowid: Some(head),
                submission_key: format!("user-goal:{}", uuid::Uuid::new_v4()),
                checkpoint: store::CheckpointInput {
                    node_id: q.node_id.clone(),
                    owner,
                    generation: q.generation,
                    version,
                    digest: goal_hash,
                    summary: q.goal.clone(),
                    source_ref: q.user_ref.clone(),
                    git_revision: None,
                },
                artifacts: vec![],
                resources: None,
            },
            crate::submission::Submitter::Operator,
        )?;
        tx.execute(
            "INSERT INTO map_goal_checkpoints(event_id,checkpoint_id) VALUES(?,?)",
            params![id, receipt.checkpoint_rowid],
        )?;
        Ok(id)
    }
}

// Repairing a route must not turn its former node back into an RC-only node.
pub(crate) fn host_managed(tx: &Transaction, node: &str) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM map_host_routes WHERE node_id=?1)
        OR EXISTS(SELECT 1 FROM map_events WHERE node_id=?1 AND kind='host_detached')",
        [node],
        |r| r.get(0),
    )?)
}

pub(crate) fn legacy_assignment(tx: &Transaction, node: &str, token: Option<&str>) -> Result<bool> {
    let token_hash = token.map(crate::identity::token_hash);
    let migrated = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM map_rc2_nodes l JOIN map_dev_nodes n
         ON (l.node_id,l.owner,l.generation)=(n.id,n.owner,n.generation)
         WHERE n.id=?1 AND (?2 IS NULL OR EXISTS(SELECT 1 FROM map_rc2_workers WHERE token_hash=?2)))",
        params![node, token_hash], |r| r.get::<_,bool>(0),
    )?;
    Ok(migrated && !host_managed(tx, node)?)
}

pub(crate) fn worker_write_gate(state: &Value, legacy: bool) -> Result<()> {
    match state["action"].as_str() {
        Some("continue") => Ok(()),
        Some("confirm_goal") if legacy && state["confirmation_event"] == 0 => Ok(()),
        Some("complete") => Err(Error::Conflict(
            "accepted checkpoint; explicit reopen required",
        )),
        _ => Err(Error::Conflict("node goal confirmation required")),
    }
}

pub(crate) fn global_worker_state(tx: &Transaction, project: &str, node: &str) -> Result<Value> {
    if !legacy_assignment(tx, node, None)? {
        return Err(Error::Conflict("node requires a scoped worker credential"));
    }
    let state = state(tx, project, node)?;
    if state["confirmation_event"] != 0 {
        return Err(Error::Conflict("node requires a scoped worker credential"));
    }
    Ok(state)
}

pub(crate) fn state(tx: &Transaction, project: &str, node: &str) -> Result<Value> {
    let confirmation = latest(tx, project, node)?;
    let base = confirmation
        .as_ref()
        .and_then(|(_, v)| v["base_checkpoint"].as_i64())
        .unwrap_or(0);
    let accepted:bool=tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM map_checkpoint_index c
          JOIN map_acceptances a ON (a.node_id,a.generation,a.version,a.digest)=(c.node_id,c.generation,c.version,c.digest)
          JOIN map_dev_nodes n ON n.id=c.node_id AND n.project_id=?1
          WHERE c.node_id=?2 AND a.decision='accepted' AND c.id>?3)",
        params![project,node,base],|r|r.get(0))?;
    let revoked = confirmation
        .as_ref()
        .is_some_and(|(_, v)| v["revoked"] == true);
    let action = if revoked {
        "confirm_goal"
    } else if accepted {
        "complete"
    } else if confirmation.is_some() {
        "continue"
    } else {
        "confirm_goal"
    };
    let (id, record) = confirmation.unwrap_or((0, Value::Null));
    Ok(
        json!({"action":action,"confirmation_event":id,"status_code":if action=="complete"{"COMPLETED"}else if action=="confirm_goal"{"CONFIRM_GOAL"}else{"CONTINUE"},"confirmation":if revoked {Value::Null}else{record},"revoked":revoked,"risks":risks(tx,project,node)?}),
    )
}

/// Trusted operator admission for a confirmed assignment in a new or existing project.
/// The candidate node and host route become visible together with the user checkpoint.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intake {
    new_project: Option<NewProject>,
    binding: crate::identity::Binding,
    token: String,
    host_scope: Option<crate::identity::HostScope>,
    new_node: Option<store::NodeInput>,
    confirmation: GoalConfirmation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewProject {
    name: String,
    source_ref: String,
    architecture_version: String,
    architecture_source: String,
    architecture_digest: String,
    adoption_ref: String,
}

impl MapStore {
    pub fn intake(&self, request: &Intake) -> Result<i64> {
        let b = &request.binding;
        let q = &request.confirmation;
        if b.role != "worker"
            || b.project_id != q.project_id
            || b.node_id.as_ref() != Some(&q.node_id)
            || b.generation != Some(q.generation)
        {
            return Err(Error::Invalid(
                "binding and confirmation must name the same assignment",
            ));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        store::require_current_schema(&tx)?;
        tx.execute_batch("SAVEPOINT admission")?;
        if let Some(p) = &request.new_project {
            if q.project_id.trim().is_empty()
                || p.name.trim().is_empty()
                || p.source_ref.trim().is_empty()
            {
                return Err(Error::Invalid("explicit project identity required"));
            }
            let created = tx.execute("INSERT INTO map_projects(id,name,source_ref,updated_at) VALUES(?,?,?,unixepoch()) ON CONFLICT(id) DO NOTHING",
                params![q.project_id,p.name,p.source_ref])?;
            if created == 1 {
                store::event(&tx, &q.project_id, None, "project", &p.source_ref)?;
                Self::adopt_tx(
                    &tx,
                    &q.project_id,
                    &p.architecture_version,
                    &p.architecture_source,
                    &p.architecture_digest,
                    "default",
                    &p.adoption_ref,
                    None,
                )?;
            } else {
                let same:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM map_projects p JOIN map_architecture_revisions a ON a.project_id=p.id WHERE p.id=? AND p.name=? AND p.source_ref=? AND a.version=? AND a.source_ref=? AND a.digest=? AND a.adoption_ref=?)",
                    params![q.project_id,p.name,p.source_ref,p.architecture_version,p.architecture_source,p.architecture_digest,p.adoption_ref],|row|row.get(0))?;
                if !same {
                    return Err(Error::Conflict(
                        "project exists with different provenance; verify the target",
                    ));
                }
            }
        }
        if let Some(node) = &request.new_node {
            if node.id != q.node_id
                || node.project_id != q.project_id
                || node.owner.as_ref() != Some(&b.owner)
                || node.generation != q.generation
            {
                return Err(Error::Invalid("candidate node mismatch"));
            }
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM map_dev_nodes WHERE id=?)",
                [&node.id],
                |r| r.get(0),
            )?;
            if !exists {
                Self::node_tx(&tx, node, None)?;
            }
            // Existing nodes retain their goal, hierarchy and history, even on retry.
        }
        let result = Self::bind_tx(&tx, &request.token, b, request.host_scope.as_ref());
        if let Err(Error::Conflict(crate::identity::HOST_CONFLICT)) = result {
            tx.execute_batch("ROLLBACK TO admission")?;
            let project_exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM map_projects WHERE id=?)",
                [&b.project_id],
                |row| row.get(0),
            )?;
            if project_exists {
                crate::identity::audit_conflict(&tx, b, request.host_scope.as_ref())?;
            } else if let Some(scope) = &request.host_scope {
                let project:String=tx.query_row("SELECT project_id FROM map_host_routes WHERE provider=? AND workspace=? AND task_id=?",params![scope.provider,scope.workspace,b.task_id],|row|row.get(0))?;
                store::event(
                    &tx,
                    &project,
                    None,
                    "host_binding_conflict",
                    "rejected first association",
                )?;
            }
            tx.commit()?;
            return Err(Error::Conflict(crate::identity::HOST_CONFLICT));
        }
        result?;
        crate::identity::authorize(
            &tx,
            &request.token,
            &q.project_id,
            Some(&q.node_id),
            "worker",
        )?;
        let id = Self::confirm_goal_tx(&tx, q)?;
        tx.commit()?;
        Ok(id)
    }
}

/// A trusted host records the actual displayed assistant turn independently of any user reply.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reminder {
    project_id: String,
    node_id: String,
    generation: i64,
    #[serde(default)]
    goal_revision: i64,
    message_ref: String,
    delivered_text: String,
    #[serde(default)]
    codes: std::collections::BTreeSet<RiskCode>,
}
impl MapStore {
    pub fn record_reminder(&self, r: &Reminder) -> Result<()> {
        if r.message_ref.trim().is_empty()
            || r.message_ref.len() > 512
            || r.delivered_text.trim().is_empty()
            || r.delivered_text.len() > 8192
        {
            return Err(Error::Invalid(
                "actual displayed text and turn reference required",
            ));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        store::require_current_schema(&tx)?;
        if !tx.query_row("SELECT EXISTS(SELECT 1 FROM map_dev_nodes WHERE project_id=? AND id=? AND generation=?)",params![r.project_id,r.node_id,r.generation],|row|row.get::<_,bool>(0))? {
            return Err(Error::Conflict("assignment mismatch"));
        }
        if latest(&tx, &r.project_id, &r.node_id)?.map_or(0, |(id, _)| id) != r.goal_revision {
            return Err(Error::Conflict("reminder goal revision changed; reread"));
        }
        let old:Option<String>=tx.query_row("SELECT detail FROM map_events WHERE project_id=? AND node_id=? AND kind='risk_reminder' AND json_extract(detail,'$.message_ref')=? AND json_extract(detail,'$.generation')=? ORDER BY id DESC LIMIT 1",params![r.project_id,r.node_id,r.message_ref,r.generation],|row|row.get(0)).optional()?;
        let mut payload = json!({"generation":r.generation,"message_ref":r.message_ref,"delivered_text":r.delivered_text});
        if r.goal_revision != 0 {
            payload["goal_revision"] = json!(r.goal_revision);
        }
        if !r.codes.is_empty() {
            payload["codes"] = json!(&r.codes);
        }
        let detail = payload.to_string();
        match old {
            Some(previous) if previous != detail => {
                return Err(Error::Conflict(
                    "turn reference already recorded with different text",
                ))
            }
            Some(_) => (),
            None => store::event(
                &tx,
                &r.project_id,
                Some(&r.node_id),
                "risk_reminder",
                &detail,
            )?,
        }
        tx.commit()?;
        Ok(())
    }
}

#[derive(Deserialize, serde::Serialize, Ord, PartialOrd, Eq, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum RiskCode {
    UseCase,
    Budget,
    Region,
    Compatibility,
    Safety,
    Compliance,
    Privacy,
    Platform,
    Deployment,
    Dependencies,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum RiskStatus {
    Unknown,
    Deferred,
    Inferred,
    UserConfirmed,
    EvidenceVerified,
    NotApplicable,
}

/// The operator attests the provenance of a single risk answer or technical observation.
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RiskRecord {
    project_id: String,
    node_id: String,
    generation: i64,
    code: RiskCode,
    status: RiskStatus,
    source_ref: String,
    detail: String,
}
impl MapStore {
    pub fn record_risk(&self, r: &RiskRecord) -> Result<()> {
        if r.source_ref.trim().is_empty()
            || r.source_ref.len() > 512
            || r.detail.trim().is_empty()
            || r.detail.len() > 2048
        {
            return Err(Error::Invalid(
                "risk observation and source reference required",
            ));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        store::require_current_schema(&tx)?;
        if !tx.query_row("SELECT EXISTS(SELECT 1 FROM map_dev_nodes WHERE project_id=? AND id=? AND generation=?)",params![r.project_id,r.node_id,r.generation],|row|row.get::<_,bool>(0))? {
            return Err(Error::Conflict("assignment mismatch"));
        }
        let value = json!(r);
        let previous:Option<String>=tx.query_row("SELECT detail FROM map_events WHERE project_id=? AND node_id=? AND kind='risk_observation' AND json_extract(detail,'$.code')=? AND json_extract(detail,'$.source_ref')=? LIMIT 1",
            params![r.project_id,r.node_id,value["code"].as_str(),r.source_ref],|row|row.get(0)).optional()?;
        match previous {
            Some(old) if old != value.to_string() => {
                return Err(Error::Conflict("risk source already recorded differently"))
            }
            Some(_) => (),
            None => store::event(
                &tx,
                &r.project_id,
                Some(&r.node_id),
                "risk_observation",
                &value.to_string(),
            )?,
        }
        tx.commit()?;
        Ok(())
    }
}
fn risks(tx: &Transaction, project: &str, node: &str) -> Result<Value> {
    let mut result = json!({});
    // A changed or withdrawn goal starts a new reminder cycle. Keep initial intake receipts.
    let cycle = latest(tx, project, node)?
        .filter(|(_, v)| v["previous"].as_i64().unwrap_or(0) > 0)
        .map_or(0, |(id, _)| id);
    let mut reminders=tx.prepare("SELECT DISTINCT j.value FROM map_events e,json_each(e.detail,'$.codes') j WHERE e.project_id=? AND e.node_id=? AND e.kind='risk_reminder' AND e.id>?")?;
    for code in reminders.query_map(params![project, node, cycle], |row| row.get::<_, String>(0))? {
        result[code?] = json!({"reminded":true,"status":"unknown"});
    }
    let mut facts=tx.prepare("SELECT detail FROM (SELECT detail, row_number() OVER(PARTITION BY json_extract(detail,'$.code') ORDER BY id DESC) AS rank FROM map_events WHERE project_id=? AND node_id=? AND kind='risk_observation' AND id>?) WHERE rank=1")?;
    for detail in facts.query_map(params![project, node, cycle], |r| r.get::<_, String>(0))? {
        let value: Value = serde_json::from_str(&detail?)
            .map_err(|_| Error::Invalid("invalid risk observation"))?;
        let code = value["code"]
            .as_str()
            .ok_or(Error::Invalid("invalid risk code"))?;
        result[code] = json!({"reminded":result[code]["reminded"]==true,"status":value["status"],"detail":value["detail"],"source_ref":value["source_ref"]});
    }
    Ok(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Decision {
    Research,
    HazardousOperation,
    Freeze,
    Procure,
    Launch,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Hardware,
    Software,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    project_id: String,
    node_id: String,
    kind: Kind,
    action: Decision,
}
impl MapStore {
    /// Read-only prerequisite check. Even an empty pending list grants no operation authority.
    pub fn assess_risk(&self, q: &Assessment) -> Result<Value> {
        use Decision::*;
        use Kind::*;
        let needed: &[&str] = match (&q.kind, &q.action) {
            (_, Research) => &[],
            (_, HazardousOperation) => &["SAFETY"],
            (Hardware, Procure) => &["BUDGET", "COMPATIBILITY", "SAFETY"],
            (Hardware, Freeze | Launch) => &[
                "USE_CASE",
                "BUDGET",
                "REGION",
                "COMPATIBILITY",
                "SAFETY",
                "COMPLIANCE",
            ],
            (Software, Procure) => &["BUDGET", "PLATFORM", "DEPENDENCIES"],
            (Software, Freeze | Launch) => &["PLATFORM", "DEPENDENCIES", "PRIVACY", "DEPLOYMENT"],
        };
        self.read(|tx| {
            store::require_current_schema(tx)?;
            if !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM map_dev_nodes WHERE project_id=? AND id=?)",
                params![q.project_id, q.node_id],
                |r| r.get::<_, bool>(0),
            )? {
                return Err(Error::Missing);
            }
            let evidence = risks(tx, &q.project_id, &q.node_id)?;
            let mut pending = Vec::new();
            for &code in needed {
                let status = evidence[code]["status"].as_str();
                let sufficient = match status {
                    Some("evidence_verified") => true,
                    Some("not_applicable") => !matches!(q.action, HazardousOperation),
                    Some("user_confirmed") => !matches!(code, "SAFETY" | "COMPLIANCE"),
                    _ => false,
                };
                if !sufficient {
                    pending.push(code);
                }
            }
            let missing = missing_reminders(&evidence, &q.kind);
            Ok(json!({"missing_reminders":missing,"pending":pending,"operation_authorized":false}))
        })
    }
}

pub fn missing_reminders(risks: &Value, kind: &Kind) -> Vec<&'static str> {
    let required: &[&str] = match kind {
        Kind::Hardware => &[
            "USE_CASE",
            "BUDGET",
            "REGION",
            "COMPATIBILITY",
            "SAFETY",
            "COMPLIANCE",
        ],
        Kind::Software => &["PLATFORM", "PRIVACY", "DEPENDENCIES", "DEPLOYMENT"],
    };
    required
        .iter()
        .copied()
        .filter(|code| risks[*code]["reminded"] != true)
        .collect()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revocation {
    project_id: String,
    node_id: String,
    previous: i64,
    user_ref: String,
}
impl MapStore {
    pub fn revoke_goal(&self, q: &Revocation) -> Result<()> {
        if q.user_ref.trim().is_empty() || q.user_ref.len() > 512 {
            return Err(Error::Invalid("actual user revocation reference required"));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        store::require_current_schema(&tx)?;
        let (id, v) = latest(&tx, &q.project_id, &q.node_id)?.ok_or(Error::Missing)?;
        if v["revoked"] == true && v["previous"] == q.previous && v["user_ref"] == q.user_ref {
            return Ok(());
        }
        if id != q.previous || v["revoked"] == true {
            return Err(Error::Conflict("confirmation changed; reread"));
        }
        store::event(
            &tx,
            &q.project_id,
            Some(&q.node_id),
            "user_goal_revoked",
            &json!({"revoked":true,"previous":id,"user_ref":q.user_ref}).to_string(),
        )?;
        tx.commit()?;
        Ok(())
    }
}
