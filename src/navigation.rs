// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
//! Rebuildable navigation and state summaries. Each query reads one indexed level.
use crate::store::{self, Error, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Clone, Serialize, Deserialize)]
pub struct Card {
    pub node_ref: String,
    pub parent_ref: Option<String>,
    pub kind: String,
    pub title: String,
    pub sort_key: String,
    pub source_ref: Option<String>,
    pub plan_state: Option<String>,
    pub development_state: Option<String>,
    pub acceptance_state: Option<String>,
    pub node_version: i64,
    pub direct_children: i64,
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Card> {
    Ok(Card {
        node_ref: r.get(0)?,
        parent_ref: r.get(1)?,
        kind: r.get(2)?,
        title: r.get(3)?,
        sort_key: r.get(4)?,
        source_ref: r.get(5)?,
        plan_state: r.get(6)?,
        development_state: r.get(7)?,
        acceptance_state: r.get(8)?,
        node_version: r.get(9)?,
        direct_children: r.get(10)?,
    })
}
const FIELDS:&str="node_ref,parent_ref,kind,substr(title,1,180),sort_key,source_ref,plan_state,development_state,acceptance_state,node_version,direct_children";
pub fn revision(tx: &Transaction<'_>, project: &str, requested: Option<&str>) -> Result<String> {
    let v = if let Some(r) = requested {
        r.to_string()
    } else {
        tx.query_row(
            "SELECT version FROM map_architectures WHERE project_id=?",
            [project],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_default()
    };
    if !v.is_empty() && !tx.query_row("SELECT EXISTS(SELECT 1 FROM map_architecture_revisions WHERE project_id=? AND version=?)",params![project,v],|r|r.get::<_,bool>(0))?{return Err(Error::Missing)}
    Ok(v)
}
pub fn card(tx: &Transaction<'_>, project: &str, rev: &str, node: &str) -> Result<Card> {
    tx.query_row(
        &format!(
            "SELECT {FIELDS} FROM map_navigation WHERE project_id=? AND revision=? AND node_ref=?"
        ),
        params![project, rev, node],
        row,
    )
    .optional()?
    .ok_or(Error::Missing)
}
fn path(tx: &Transaction<'_>, project: &str, rev: &str, node: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut next = Some(node.to_string());
    let mut seen = HashSet::new();
    while let Some(id) = next {
        if !seen.insert(id.clone()) {
            return Err(Error::Conflict("navigation cycle"));
        }
        let p:Option<Option<String>>=tx.query_row("SELECT parent_ref FROM map_navigation WHERE project_id=? AND revision=? AND node_ref=?",params![project,rev,id],|r|r.get(0)).optional()?;
        let Some(parent) = p else { break };
        out.push(id);
        next = parent;
    }
    Ok(out)
}
fn refresh_rollup(
    tx: &Transaction<'_>,
    project: &str,
    rev: &str,
    node: &str,
    event: i64,
) -> Result<()> {
    tx.execute(
        "DELETE FROM map_rollups WHERE project_id=? AND revision=? AND node_ref=?",
        params![project, rev, node],
    )?;
    tx.execute("INSERT INTO map_rollups SELECT ?,?,?,node_kind,SUM(total),SUM(planned),SUM(active),SUM(blocked),SUM(paused),SUM(submitted),SUM(pending),SUM(accepted),SUM(stale),SUM(rejected),? FROM (
 SELECT kind AS node_kind,1 AS total,COALESCE(plan_state='planned',0) AS planned,COALESCE(plan_state='active',0) AS active,COALESCE(plan_state='blocked',0) AS blocked,COALESCE(plan_state='paused',0) AS paused,COALESCE(development_state='submitted',0) AS submitted,COALESCE(acceptance_state='pending',0) AS pending,COALESCE(acceptance_state='accepted',0) AS accepted,COALESCE(acceptance_state='stale',0) AS stale,COALESCE(acceptance_state='rejected',0) AS rejected FROM map_navigation WHERE project_id=? AND revision=? AND node_ref=?
 UNION ALL SELECT r.node_kind,r.total,r.planned,r.active,r.blocked,r.paused,r.submitted,r.pending,r.accepted,r.stale,r.rejected FROM map_navigation c JOIN map_rollups r ON r.project_id=c.project_id AND r.revision=c.revision AND r.node_ref=c.node_ref WHERE c.project_id=? AND c.revision=? AND c.parent_ref=?) GROUP BY node_kind",params![project,rev,node,event,project,rev,node,project,rev,node])?;
    tx.execute("UPDATE map_navigation SET direct_children=(SELECT COUNT(*) FROM map_navigation c WHERE c.project_id=? AND c.revision=? AND c.parent_ref=?),node_version=? WHERE project_id=? AND revision=? AND node_ref=?",params![project,rev,node,event,project,rev,node])?;
    Ok(())
}
fn upsert(tx: &Transaction<'_>, project: &str, rev: &str, c: &Card, event: i64) -> Result<()> {
    tx.execute("INSERT INTO map_navigation(project_id,revision,node_ref,parent_ref,kind,title,sort_key,source_ref,plan_state,development_state,acceptance_state,node_version) VALUES(?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(project_id,revision,node_ref) DO UPDATE SET parent_ref=excluded.parent_ref,kind=excluded.kind,title=excluded.title,sort_key=excluded.sort_key,source_ref=excluded.source_ref,plan_state=excluded.plan_state,development_state=excluded.development_state,acceptance_state=excluded.acceptance_state,node_version=excluded.node_version",params![project,rev,c.node_ref,c.parent_ref,c.kind,c.title,c.sort_key,c.source_ref,c.plan_state,c.development_state,c.acceptance_state,event])?;
    Ok(())
}
fn simple(
    node_ref: String,
    parent_ref: Option<String>,
    kind: String,
    title: String,
    sort_key: String,
    source: Option<String>,
) -> Card {
    Card {
        node_ref,
        parent_ref,
        kind,
        title,
        sort_key,
        source_ref: source,
        plan_state: None,
        development_state: None,
        acceptance_state: None,
        node_version: 0,
        direct_children: 0,
    }
}
fn dev_card(tx: &Transaction<'_>, project: &str, rev: &str, id: &str) -> Result<Card> {
    let (parent,kind,title,sort,source,plan):(Option<String>,String,String,String,Option<String>,String)=tx.query_row("SELECT parent_id,kind,title,sort_key,architecture_ref,plan_state FROM map_dev_nodes WHERE id=? AND project_id=?",params![id,project],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?.ok_or(Error::Missing)?;
    let parent_ref = if let Some(p) = parent {
        format!("dev:{p}")
    } else {
        let primary:Option<String>=tx.query_row("SELECT t.arch_node_id FROM map_local_traces t JOIN map_arch_nodes a ON a.id=t.arch_node_id AND a.arch_version=t.arch_version WHERE t.dev_node_id=? AND t.arch_version=? AND a.project_id=? AND t.relation='primary' ORDER BY t.arch_node_id LIMIT 1",params![id,rev,project],|r|r.get(0)).optional()?;
        primary.map(|p| format!("arch:{p}")).unwrap_or_else(|| {
            if rev.is_empty() {
                format!("project:{project}")
            } else {
                format!("architecture:{project}:{rev}")
            }
        })
    };
    let latest:Option<(i64,i64,String,String)>=tx.query_row("SELECT generation,version,digest,arch_version FROM map_checkpoint_index WHERE node_id=? ORDER BY generation DESC,version DESC LIMIT 1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let acceptance = if let Some((g, v, d, arch)) = &latest {
        let a:Option<(String,String)>=tx.query_row("SELECT digest,decision FROM map_acceptances WHERE node_id=? AND generation=? AND version=?",params![id,g,v],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let assigned_generation: i64 = tx.query_row(
            "SELECT generation FROM map_dev_nodes WHERE id=?",
            [id],
            |r| r.get(0),
        )?;
        if arch != rev || *g != assigned_generation {
            "stale".to_string()
        } else if let Some((ad, decision)) = a {
            if ad == *d {
                decision
            } else {
                "stale".into()
            }
        } else {
            let old: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM map_acceptances WHERE node_id=?)",
                [id],
                |r| r.get(0),
            )?;
            if old {
                "stale".into()
            } else {
                "pending".into()
            }
        }
    } else {
        "pending".into()
    };
    let mut c = simple(
        format!("dev:{id}"),
        Some(parent_ref),
        kind,
        title,
        sort,
        source,
    );
    c.plan_state = Some(plan);
    c.development_state = Some(
        if latest.is_some() {
            "submitted"
        } else {
            "none"
        }
        .into(),
    );
    c.acceptance_state = Some(acceptance);
    Ok(c)
}
fn build_revision(tx: &Transaction<'_>, project: &str, rev: &str, event: i64) -> Result<()> {
    tx.execute(
        "DELETE FROM map_navigation WHERE project_id=? AND revision=?",
        params![project, rev],
    )?;
    tx.execute(
        "DELETE FROM map_rollups WHERE project_id=? AND revision=?",
        params![project, rev],
    )?;
    let (name, source): (String, String) = tx.query_row(
        "SELECT name,source_ref FROM map_projects WHERE id=?",
        [project],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    upsert(
        tx,
        project,
        rev,
        &simple(
            format!("project:{project}"),
            None,
            "project".into(),
            name,
            project.into(),
            Some(source),
        ),
        event,
    )?;
    if !rev.is_empty() {
        let source: String = tx.query_row(
            "SELECT source_ref FROM map_architecture_revisions WHERE project_id=? AND version=?",
            params![project, rev],
            |r| r.get(0),
        )?;
        upsert(
            tx,
            project,
            rev,
            &simple(
                format!("architecture:{project}:{rev}"),
                Some(format!("project:{project}")),
                "architecture".into(),
                format!("核心架构 {rev}"),
                rev.into(),
                Some(source),
            ),
            event,
        )?;
        let mut q=tx.prepare("SELECT id,parent_id,title,kind,source_ref,sort_key FROM map_arch_nodes WHERE project_id=? AND arch_version=?")?;
        let nodes = q
            .query_map(params![project, rev], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, parent, title, kind, source, sort) in nodes {
            upsert(
                tx,
                project,
                rev,
                &simple(
                    format!("arch:{id}"),
                    Some(
                        parent
                            .map(|p| format!("arch:{p}"))
                            .unwrap_or_else(|| format!("architecture:{project}:{rev}")),
                    ),
                    kind,
                    title,
                    sort,
                    Some(source),
                ),
                event,
            )?;
        }
    }
    let mut q = tx.prepare("SELECT id FROM map_dev_nodes WHERE project_id=?")?;
    let ids = q
        .query_map([project], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        upsert(tx, project, rev, &dev_card(tx, project, rev, &id)?, event)?;
    }
    let mut q=tx.prepare("WITH RECURSIVE tree(node_ref,depth) AS (SELECT node_ref,0 FROM map_navigation WHERE project_id=? AND revision=? AND parent_ref IS NULL UNION ALL SELECT n.node_ref,t.depth+1 FROM map_navigation n JOIN tree t ON n.parent_ref=t.node_ref WHERE n.project_id=? AND n.revision=?) SELECT node_ref FROM tree ORDER BY depth DESC")?;
    let ids = q
        .query_map(params![project, rev, project, rev], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM map_navigation WHERE project_id=? AND revision=?",
        params![project, rev],
        |r| r.get(0),
    )?;
    if ids.len() != count as usize {
        return Err(Error::Conflict("unreachable navigation node or cycle"));
    }
    for id in ids {
        refresh_rollup(tx, project, rev, &id, event)?;
    }
    Ok(())
}
pub fn rebuild_project(tx: &Transaction<'_>, project: &str) -> Result<()> {
    let event: i64 = tx.query_row(
        "SELECT COALESCE(MAX(id),0) FROM map_events WHERE project_id=?",
        [project],
        |r| r.get(0),
    )?;
    let mut q = tx.prepare("SELECT version FROM map_architecture_revisions WHERE project_id=?")?;
    let mut revs = q
        .query_map([project], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if revs.is_empty() {
        revs.push(String::new())
    }
    tx.execute("DELETE FROM map_navigation WHERE project_id=?", [project])?;
    tx.execute("DELETE FROM map_rollups WHERE project_id=?", [project])?;
    for rev in revs {
        build_revision(tx, project, &rev, event)?;
    }
    Ok(())
}
pub fn rebuild(tx: &Transaction<'_>) -> Result<()> {
    let mut q = tx.prepare("SELECT id FROM map_projects ORDER BY id")?;
    let ids = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        rebuild_project(tx, &id)?;
    }
    Ok(())
}
pub(crate) fn changed(
    tx: &Transaction<'_>,
    project: &str,
    node: Option<&str>,
    kind: &str,
) -> Result<()> {
    // New baselines or components change the structural projection. Ordinary submissions only touch the node and its ancestors.
    if node.is_none() || matches!(kind, "architecture_trace") {
        return rebuild_project(tx, project);
    }
    let id = node.unwrap();
    let event: i64 = tx.query_row(
        "SELECT COALESCE(MAX(id),0) FROM map_events WHERE project_id=?",
        [project],
        |r| r.get(0),
    )?;
    let mut q = tx.prepare("SELECT DISTINCT revision FROM map_navigation WHERE project_id=?")?;
    let revs = q
        .query_map([project], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if revs.is_empty() {
        return rebuild_project(tx, project);
    }
    for rev in revs {
        let node_ref = format!("dev:{id}");
        let old_path = path(tx, project, &rev, &node_ref)?;
        upsert(tx, project, &rev, &dev_card(tx, project, &rev, id)?, event)?;
        // First subtract moved subtrees from their old ancestry, then add to the new ancestry.
        for a in old_path.iter().skip(1) {
            refresh_rollup(tx, project, &rev, a, event)?;
        }
        for a in path(tx, project, &rev, &node_ref)? {
            refresh_rollup(tx, project, &rev, &a, event)?;
        }
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct Cursor {
    project: String,
    revision: String,
    parent: String,
    sort: String,
    node: String,
    version: i64,
    state: Option<String>,
}
pub fn children(
    tx: &Transaction<'_>,
    project: &str,
    requested: Option<&str>,
    parent: &str,
    after: &str,
    n: usize,
    state: Option<&str>,
) -> Result<Value> {
    let n = store::limit(Some(n));
    let rev = revision(tx, project, requested)?;
    let parent_card = card(tx, project, &rev, parent)?;
    let (sort, last) = if after.is_empty() {
        (String::new(), String::new())
    } else {
        let c: Cursor =
            serde_json::from_str(after).map_err(|_| Error::Invalid("navigation cursor"))?;
        if c.project != project
            || c.revision != rev
            || c.parent != parent
            || c.state.as_deref() != state
        {
            return Err(Error::Invalid("cursor belongs to another view"));
        }
        if c.version != parent_card.node_version {
            return Err(Error::Conflict("navigation changed; reload this branch"));
        }
        (c.sort, c.node)
    };
    let sql=format!("SELECT {FIELDS} FROM map_navigation WHERE project_id=? AND revision=? AND parent_ref=? AND (sort_key,node_ref)>(?,?) AND (? IS NULL OR plan_state=?) ORDER BY sort_key,node_ref LIMIT ?");
    let mut q = tx.prepare(&sql)?;
    let mut items = q
        .query_map(
            params![
                project,
                rev,
                parent,
                sort,
                last,
                state,
                state,
                (store::limit(Some(n)) + 1) as i64
            ],
            row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let more = items.len() > n;
    items.truncate(n);
    let next = if more {
        items.last().map(|c| {
            serde_json::to_string(&Cursor {
                project: project.into(),
                revision: rev.clone(),
                parent: parent.into(),
                sort: c.sort_key.clone(),
                node: c.node_ref.clone(),
                version: parent_card.node_version,
                state: state.map(str::to_string),
            })
            .unwrap()
        })
    } else {
        None
    };
    Ok(
        json!({"project_id":project,"revision":rev,"parent_ref":parent,"node_version":parent_card.node_version,"items":items,"next":next,"has_more":more,"development_plan_scope":"current"}),
    )
}
pub fn ancestors(
    tx: &Transaction<'_>,
    project: &str,
    rev: &str,
    node: &str,
    after: Option<&str>,
    n: usize,
) -> Result<Value> {
    let version = card(tx, project, rev, &format!("project:{project}"))?.node_version;
    let mut current = if let Some(after) = after {
        let cursor: (String, String, String, i64, String) =
            serde_json::from_str(after).map_err(|_| Error::Invalid("ancestor cursor"))?;
        if cursor.0 != project || cursor.1 != rev || cursor.2 != node {
            return Err(Error::Invalid("ancestor cursor scope"));
        }
        if cursor.3 != version {
            return Err(Error::Conflict("ancestry changed; reload navigation"));
        }
        if !path(tx, project, rev, node)?.contains(&cursor.4) {
            return Err(Error::Invalid("ancestor cursor outside path"));
        }
        cursor.4
    } else {
        node.to_string()
    };
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for _ in 0..store::limit(Some(n)) {
        if !seen.insert(current.clone()) {
            return Err(Error::Conflict("navigation cycle"));
        }
        let c = card(tx, project, rev, &current)?;
        let parent = c.parent_ref.clone();
        items.push(
            json!({"node_ref":c.node_ref,"kind":c.kind,"title":c.title,"parent_ref":c.parent_ref}),
        );
        match parent {
            Some(p) => current = p,
            None => return Ok(json!({"items":items,"next":null})),
        }
    }
    let next = serde_json::to_string(&(project, rev, node, version, current)).unwrap();
    Ok(json!({"items":items,"next":next}))
}
pub fn focus(
    tx: &Transaction<'_>,
    project: &str,
    requested: Option<&str>,
    node: &str,
) -> Result<Value> {
    let rev = revision(tx, project, requested)?;
    let c = card(tx, project, &rev, node)?;
    let a = ancestors(tx, project, &rev, node, None, 100)?;
    let mut breadcrumbs = a["items"].as_array().unwrap().clone();
    breadcrumbs.reverse();
    let current: Option<String> = tx
        .query_row(
            "SELECT version FROM map_architectures WHERE project_id=?",
            [project],
            |r| r.get(0),
        )
        .optional()?;
    let event: i64 = tx.query_row(
        "SELECT COALESCE(MAX(id),0) FROM map_events WHERE project_id=?",
        [project],
        |r| r.get(0),
    )?;
    let core = if rev.is_empty() {
        tx.query_row(
            "SELECT 'dev:'||core_node FROM map_local_sources WHERE project_id=?",
            [project],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    } else {
        Some(format!("architecture:{project}:{rev}"))
    };
    Ok(
        json!({"project_id":project,"revision":rev,"current_revision":current,"node":c,"parent_ref":c.parent_ref,"breadcrumbs":breadcrumbs,"ancestors_next":a["next"],"project_ref":format!("project:{project}"),"core_architecture_ref":core,"snapshot_event_id":event,"view_key":[project,rev,node],"development_plan_scope":"current","deep_link":format!("?project_id={}&revision={}&node_ref={}",encode(project),encode(&rev),encode(node))}),
    )
}
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
pub fn summary(
    tx: &Transaction<'_>,
    project: &str,
    requested: Option<&str>,
    node: &str,
) -> Result<Value> {
    let rev = revision(tx, project, requested)?;
    let c = card(tx, project, &rev, node)?;
    let mut q=tx.prepare("SELECT node_kind,total,planned,active,blocked,paused,submitted,pending,accepted,stale,rejected,event_id FROM map_rollups WHERE project_id=? AND revision=? AND node_ref=? ORDER BY node_kind")?;
    let items=q.query_map(params![project,rev,node],|r|Ok(json!({"kind":r.get::<_,String>(0)?,"total":r.get::<_,i64>(1)?,"planned":r.get::<_,i64>(2)?,"active":r.get::<_,i64>(3)?,"blocked":r.get::<_,i64>(4)?,"paused":r.get::<_,i64>(5)?,"submitted":r.get::<_,i64>(6)?,"pending":r.get::<_,i64>(7)?,"accepted":r.get::<_,i64>(8)?,"stale":r.get::<_,i64>(9)?,"rejected":r.get::<_,i64>(10)?,"event_id":r.get::<_,i64>(11)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(
        json!({"project_id":project,"revision":rev,"node_ref":node,"direct_children":c.direct_children,"by_kind":items,"includes_focus":true}),
    )
}
