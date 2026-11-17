//! Agent-reported scope reminders. Not a sandbox, identity proof or new permission grant.
use crate::{
    identity,
    store::{self, Error, MapStore, Result},
    takeover,
};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct ScopeItem {
    pub action: String,
    pub project: String,
    pub resource: String,
    pub scope: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeRequest {
    pub project_id: String,
    pub node_id: String,
    pub operations: Vec<ScopeItem>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeReport {
    pub project_id: String,
    pub node_id: String,
    pub goal_revision: i64,
    pub scope_revision: i64,
    pub approved: bool,
    pub scope: Vec<ScopeItem>,
    pub user_reply: Option<String>,
    pub message_ref: Option<String>,
    pub note: String,
}
fn canonical(items: &[ScopeItem]) -> Result<Vec<ScopeItem>> {
    if items.len() > 64 {
        return Err(Error::Invalid("scope exceeds 64 entries"));
    }
    let mut result = BTreeSet::new();
    for x in items {
        if [&x.action, &x.project, &x.resource, &x.scope]
            .iter()
            .any(|s| {
                s.trim().is_empty()
                    || s.len() > 256
                    || s.contains('*')
                    || s.chars().any(char::is_control)
            })
        {
            return Err(Error::Invalid(
                "explicit bounded scope labels required; no wildcard",
            ));
        }
        let mut x = x.clone();
        if x.scope == "implementation"
            && [
                "develop",
                "implement",
                "refactor",
                "migrate",
                "fix",
                "test",
                "translate",
                "replace",
                "debug",
                "measure",
                "build",
                "format",
                "lint",
            ]
            .contains(&x.action.as_str())
        {
            x.action = "develop".into();
        }
        result.insert(x);
    }
    Ok(result.into_iter().collect())
}
/// Caller must already have authorized this project/node. Reads only current goal's scope.
pub(crate) fn snapshot(tx: &Transaction, project: &str, node: &str) -> Result<Value> {
    let goal = takeover::state(tx, project, node)?;
    let revision = goal["confirmation_event"].as_i64().unwrap_or(0);
    let row:Option<(i64,String)>=tx.query_row("SELECT id,detail FROM map_events WHERE project_id=? AND node_id=? AND kind='agent_scope_decision' AND json_extract(detail,'$.goal_revision')=? ORDER BY id DESC LIMIT 1",params![project,node,revision],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let (id, scope, context) = match row {
        Some((id, v)) => {
            let v: Value = serde_json::from_str(&v).map_err(|_| Error::Invalid("scope record"))?;
            (id, v["scope"].clone(), v)
        }
        None => (0, json!([]), json!({})),
    };
    Ok(
        json!({"project_id":project,"node_id":node,"goal_revision":revision,"scope_revision":id,"scope":scope,"note":context["scope_note"],"user_reply":context["scope_user_reply"],"last_decision":context["approved"],"last_decision_note":context["note"],"last_decision_user_reply":context["user_reply"],"task_action":goal["action"],"trust":"agent_reported_user_decision","identity_verified":false,"security_boundary":false}),
    )
}
fn ready(snapshot: &Value) -> bool {
    snapshot["task_action"] == "continue"
}
impl MapStore {
    pub fn scope_check(&self, token: &str, q: &ScopeRequest) -> Result<Value> {
        let operations = canonical(&q.operations)?;
        self.read(|tx|{store::require_current_schema(tx)?;identity::authorize(tx,token,&q.project_id,Some(&q.node_id),"reader")?;
   let current=snapshot(tx,&q.project_id,&q.node_id)?;
   if !ready(&current){return Ok(json!({"status":"BLOCKED","reason":"existing goal confirmation/revocation/accepted rules apply","current":current,"security_boundary":false}));}
   let known:Vec<ScopeItem>=serde_json::from_value(current["scope"].clone()).map_err(|_|Error::Invalid("scope record"))?;
   let differences:Vec<_>=operations.iter().filter(|x|!known.contains(x)).cloned().collect();
   let suggested_scope:Vec<_>=known.iter().chain(operations.iter()).cloned().collect::<BTreeSet<_>>().into_iter().collect();
   if suggested_scope.len()>64 {return Ok(json!({"status":"BLOCKED","reason":"SCOPE_CAPACITY_EXCEEDED","capacity":{"limit":64,"required":suggested_scope.len()},"scope_difference":differences,"suggested_scope":null,"current":current,"required_user_action":null,"hint":"Do not request approval for an unrecordable union or silently truncate it. Existing in-scope work remains available; record a smaller set only if it matches the user's actual boundaries.","security_boundary":false,"identity_verified":false}));}
   let unknown=operations.is_empty();
   Ok(json!({"status":if unknown||!differences.is_empty(){"NEEDS_USER_CONFIRMATION"}else{"IN_SCOPE"},"reason":if unknown{"no explicit operation scope; cannot judge"}else if !differences.is_empty(){"declared operation tuples exceed saved scope"}else{"declared operation is within Agent-reported scope"},"scope_difference":differences,"suggested_scope":suggested_scope,"current":current,"required_user_action":if unknown||!differences.is_empty(){json!("Explain why, affected modules/projects/resources, risk/impact, and whether original goal is possible without expansion; ask the user. Pause only the extra work.")}else{Value::Null},"security_boundary":false,"identity_verified":false}))
  })
    }
    /// Records an Agent's true/false report. Never confirms/revokes/reopens a Core goal.
    pub fn scope_report(&self, token: &str, q: &ScopeReport) -> Result<Value> {
        let scope = canonical(&q.scope)?;
        if (q.approved && scope.is_empty())
            || q.note.len() > 4096
            || q.user_reply.as_ref().is_some_and(|s| s.len() > 8192)
            || q.message_ref.as_ref().is_some_and(|s| s.len() > 1024)
        {
            return Err(Error::Invalid(
                "bounded decision and explicit approved scope required",
            ));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        store::require_current_schema(&tx)?;
        let b = identity::authorize(&tx, token, &q.project_id, Some(&q.node_id), "reader")?;
        if !["worker", "manager"].contains(&b.role.as_str()) {
            return Err(Error::Invalid(
                "scope report requires current worker or manager",
            ));
        }
        let current = snapshot(&tx, &q.project_id, &q.node_id)?;
        if !ready(&current) {
            return Err(Error::Conflict(
                "scope report cannot confirm revoked/unconfirmed goals or reopen accepted tasks",
            ));
        }
        if current["goal_revision"] != q.goal_revision
            || current["scope_revision"] != q.scope_revision
        {
            return Err(Error::Conflict(
                "goal/scope changed; reread current scope and user decision",
            ));
        }
        // A true report replaces the complete effective scope with precisely the reported set.
        // This also supports user reductions without silently unioning the old larger scope.
        let effective = if q.approved {
            json!(scope)
        } else {
            current["scope"].clone()
        };
        let record = json!({"approved":q.approved,"goal_revision":q.goal_revision,"previous_scope_revision":q.scope_revision,"scope":effective,"proposed_scope":scope,"scope_note":if q.approved {json!(q.note)}else{current["note"].clone()},"scope_user_reply":if q.approved {json!(q.user_reply)}else{current["user_reply"].clone()},"user_reply":q.user_reply,"message_ref":q.message_ref,"note":q.note,"reporting_subject":b.subject,"trust":"agent_reported_user_decision","identity_verified":false});
        store::event(
            &tx,
            &q.project_id,
            Some(&q.node_id),
            "agent_scope_decision",
            &record.to_string(),
        )?;
        let result = snapshot(&tx, &q.project_id, &q.node_id)?;
        tx.commit()?;
        Ok(
            json!({"status":if q.approved{"SCOPE_RECORDED"}else{"DECLINED_SCOPE_UNCHANGED"},"current":result,"security_boundary":false,"identity_verified":false}),
        )
    }
}
