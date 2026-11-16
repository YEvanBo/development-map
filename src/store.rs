// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAX_LIMIT: usize = 100;
#[derive(Clone)]
pub struct MapStore {
    pub(crate) path: PathBuf,
}
#[derive(Debug)]
pub enum Error {
    Db(rusqlite::Error),
    Invalid(&'static str),
    Conflict(&'static str),
    Missing,
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeInput {
    pub id: String,
    pub project_id: String,
    pub parent_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub goal: String,
    pub sort_key: String,
    pub plan_state: String,
    pub owner: Option<String>,
    pub generation: i64,
    pub architecture_ref: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointInput {
    pub node_id: String,
    pub owner: String,
    pub generation: i64,
    pub version: i64,
    pub digest: String,
    pub summary: String,
    pub source_ref: String,
    pub git_revision: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactInput {
    pub node_id: String,
    pub repo_key: String,
    pub revision: String,
    pub path: String,
    pub kind: String,
    pub provenance: String,
    pub verification: String,
    pub content_digest: Option<String>,
}

impl MapStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_owned(),
        }
    }
    pub fn connect(&self, readonly: bool) -> Result<Connection> {
        let flags = if readonly {
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
        };
        let db = Connection::open_with_flags(&self.path, flags)?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        Ok(db)
    }
    pub fn migrate(&self) -> Result<()> {
        crate::recovery::migrate(self)
    }
    pub fn import_legacy(&self) -> Result<usize> {
        let mut db = self.connect(false)?;
        if !table_exists(&db, "project_nodes")? || !table_exists(&db, "projects")? {
            return Ok(0);
        }
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("INSERT OR IGNORE INTO map_projects(id,name,source_ref,updated_at) SELECT id,name,source_ref,updated_at FROM projects", [])?;
        let n = tx.execute("INSERT OR IGNORE INTO map_dev_nodes(id,project_id,parent_id,kind,title,goal,sort_key,plan_state,architecture_ref,owner,generation,updated_at) SELECT id,project_id,parent_id,'module',title,goal,title,'legacy',NULL,NULL,0,updated_at FROM project_nodes", [])?;
        crate::navigation::rebuild(&tx)?;
        tx.commit()?;
        Ok(n)
    }
    pub fn project(&self, id: &str, name: &str, source_ref: &str) -> Result<()> {
        if id.is_empty() || name.is_empty() {
            return Err(Error::Invalid("project"));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO map_projects(id,name,source_ref,updated_at) VALUES(?,?,?,unixepoch()) ON CONFLICT(id) DO UPDATE SET name=excluded.name,source_ref=excluded.source_ref,updated_at=excluded.updated_at", params![id,name,source_ref])?;
        event(&tx, id, None, "project", source_ref)?;
        tx.commit()?;
        Ok(())
    }
    pub fn adopt_architecture(
        &self,
        project: &str,
        version: &str,
        source: &str,
        digest: &str,
    ) -> Result<()> {
        self.adopt_architecture_full(project, version, source, digest, "default", source)
    }
    pub fn adopt_architecture_full(
        &self,
        project: &str,
        version: &str,
        source: &str,
        digest: &str,
        variant: &str,
        adoption_ref: &str,
    ) -> Result<()> {
        self.adopt_bound(
            project,
            version,
            source,
            digest,
            variant,
            adoption_ref,
            None,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn adopt_bound(
        &self,
        project: &str,
        version: &str,
        source: &str,
        digest: &str,
        variant: &str,
        adoption_ref: &str,
        token: Option<&str>,
    ) -> Result<()> {
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        Self::adopt_tx(
            &tx,
            project,
            version,
            source,
            digest,
            variant,
            adoption_ref,
            token,
        )?;
        tx.commit()?;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn adopt_tx(
        tx: &Transaction,
        project: &str,
        version: &str,
        source: &str,
        digest: &str,
        variant: &str,
        adoption_ref: &str,
        token: Option<&str>,
    ) -> Result<()> {
        if version.is_empty()
            || source.is_empty()
            || digest.len() < 16
            || variant.is_empty()
            || adoption_ref.is_empty()
        {
            return Err(Error::Invalid("architecture"));
        }
        if let Some(token) = token {
            crate::identity::authorize(tx, token, project, None, "manager")?;
        }
        if !exists(tx, "map_projects", project)? {
            return Err(Error::Missing);
        }
        let previous: Option<String> = tx
            .query_row(
                "SELECT digest FROM map_architecture_revisions WHERE project_id=? AND version=?",
                params![project, version],
                |r| r.get(0),
            )
            .optional()?;
        if previous.is_some_and(|d| d != digest) {
            return Err(Error::Conflict("architecture revision digest"));
        }
        let active: Option<(String, String, String)> = tx
            .query_row(
                "SELECT version,source_ref,digest FROM map_architectures WHERE project_id=?",
                [project],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        tx.execute("INSERT OR IGNORE INTO map_architecture_revisions(project_id,version,source_ref,digest,variant,adoption_ref,adopted_at) VALUES(?,?,?,?,?,?,unixepoch())",params![project,version,source,digest,variant,adoption_ref])?;
        tx.execute("INSERT INTO map_architectures(project_id,version,source_ref,digest,adopted_at) VALUES(?,?,?,?,unixepoch()) ON CONFLICT(project_id) DO UPDATE SET version=excluded.version,source_ref=excluded.source_ref,digest=excluded.digest,adopted_at=excluded.adopted_at",params![project,version,source,digest])?;
        if active.as_ref() != Some(&(version.to_string(), source.to_string(), digest.to_string())) {
            event(tx, project, None, "architecture", version)?;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub fn architecture_node(
        &self,
        id: &str,
        project: &str,
        parent: Option<&str>,
        title: &str,
        kind: &str,
        source: &str,
        sort: &str,
    ) -> Result<()> {
        self.architecture_node_full(id, project, parent, title, kind, source, sort, None, true)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn architecture_node_full(
        &self,
        id: &str,
        project: &str,
        parent: Option<&str>,
        title: &str,
        kind: &str,
        source: &str,
        sort: &str,
        requirement_id: Option<&str>,
        required: bool,
    ) -> Result<()> {
        if id.is_empty()
            || title.is_empty()
            || source.is_empty()
            || !matches!(
                kind,
                "system" | "domain" | "component" | "interface" | "function"
            )
        {
            return Err(Error::Invalid("architecture node"));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if !exists_architecture(&tx, project)? {
            return Err(Error::Conflict("project architecture missing"));
        }
        let version: String = tx.query_row(
            "SELECT version FROM map_architectures WHERE project_id=?",
            [project],
            |r| r.get(0),
        )?;
        let id_project: Option<String> = tx
            .query_row(
                "SELECT project_id FROM map_arch_nodes WHERE id=? LIMIT 1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if id_project.is_some_and(|p| p != project) {
            return Err(Error::Conflict(
                "architecture ID belongs to another project",
            ));
        }
        if let Some(p) = parent {
            let pp: Option<String> = tx.query_row("SELECT a.project_id FROM map_arch_nodes a JOIN map_architectures v ON v.project_id=a.project_id AND v.version=a.arch_version WHERE a.id=? AND arch_version=?",params![p,version],|r|r.get(0)).optional()?;
            if pp.as_deref() != Some(project) {
                return Err(Error::Invalid("architecture parent"));
            }
            let cycle:i64=tx.query_row("WITH RECURSIVE a(id,parent_id) AS (SELECT id,parent_id FROM map_arch_nodes WHERE id=? AND arch_version=? UNION ALL SELECT p.id,p.parent_id FROM map_arch_nodes p JOIN a ON p.id=a.parent_id AND p.arch_version=?) SELECT EXISTS(SELECT 1 FROM a WHERE id=?)",params![p,version,version,id],|r|r.get(0))?;
            if cycle != 0 {
                return Err(Error::Invalid("architecture cycle"));
            }
        }
        tx.execute("INSERT INTO map_arch_nodes(id,project_id,arch_version,parent_id,title,kind,source_ref,sort_key,requirement_id,required) VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id,arch_version) DO UPDATE SET parent_id=excluded.parent_id,title=excluded.title,kind=excluded.kind,source_ref=excluded.source_ref,sort_key=excluded.sort_key,requirement_id=excluded.requirement_id,required=excluded.required",params![id,project,version,parent,title,kind,source,sort,requirement_id,required as i64])?;
        event(&tx, project, None, "architecture_node", id)?;
        tx.commit()?;
        Ok(())
    }
    pub fn architecture_edge(&self, from: &str, to: &str, kind: &str, source: &str) -> Result<()> {
        if from == to || !matches!(kind, "interface" | "requires") || source.is_empty() {
            return Err(Error::Invalid("architecture edge"));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        arch_project(&tx, from, None)?;
        arch_project(&tx, to, None)?;
        let a: Option<(String, String)> = tx
            .query_row(
                "SELECT a.project_id,a.arch_version FROM map_arch_nodes a JOIN map_architectures v ON v.project_id=a.project_id AND v.version=a.arch_version WHERE a.id=?",
                [from],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let b: Option<(String, String)> = tx
            .query_row(
                "SELECT a.project_id,a.arch_version FROM map_arch_nodes a JOIN map_architectures v ON v.project_id=a.project_id AND v.version=a.arch_version WHERE a.id=?",
                [to],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if a.is_none() || a != b {
            return Err(Error::Invalid("architecture edge scope"));
        }
        let inserted = tx.execute("INSERT OR IGNORE INTO map_architecture_edges(from_id,to_id,arch_version,kind,source_ref) VALUES(?,?,?,?,?)",params![from,to,a.as_ref().unwrap().1,kind,source])?;
        if inserted > 0 {
            event(&tx, &a.unwrap().0, None, "architecture_edge", from)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn trace(&self, arch: &str, dev: &str, relation: &str, source: &str) -> Result<()> {
        self.trace_bound(arch, dev, relation, source, None)
    }
    pub fn trace_bound(
        &self,
        arch: &str,
        dev: &str,
        relation: &str,
        source: &str,
        token: Option<&str>,
    ) -> Result<()> {
        if !matches!(relation, "primary" | "implements" | "supports" | "tests") || source.is_empty()
        {
            return Err(Error::Invalid("trace"));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        arch_project(&tx, arch, None)?;
        let ap: Option<String> = tx
            .query_row(
                "SELECT a.project_id FROM map_arch_nodes a JOIN map_architectures v ON v.project_id=a.project_id AND v.version=a.arch_version WHERE a.id=?",
                [arch],
                |r| r.get(0),
            )
            .optional()?;
        let dp: Option<String> = tx
            .query_row(
                "SELECT project_id FROM map_dev_nodes WHERE id=?",
                [dev],
                |r| r.get(0),
            )
            .optional()?;
        if ap.is_none() || ap != dp {
            return Err(Error::Invalid("trace project"));
        }
        if let Some(token) = token {
            crate::identity::authorize(&tx, token, ap.as_deref().unwrap(), Some(dev), "manager")?;
        }
        let version: String = tx.query_row(
            "SELECT version FROM map_architectures WHERE project_id=?",
            [ap.as_deref().unwrap()],
            |r| r.get(0),
        )?;
        if relation == "primary" {
            let old:Option<String>=tx.query_row("SELECT arch_node_id FROM map_local_traces WHERE dev_node_id=? AND arch_version=? AND relation='primary'",params![dev,version],|r|r.get(0)).optional()?;
            if old.is_some_and(|x| x != arch) {
                return Err(Error::Conflict("primary navigation already set"));
            }
        }
        let inserted = tx.execute("INSERT OR IGNORE INTO map_arch_traces(arch_node_id,arch_version,dev_node_id,relation,source_ref) VALUES(?,?,?,?,?)",params![arch,version,dev,relation,source])?;
        if inserted > 0 {
            event(
                &tx,
                ap.as_deref().unwrap(),
                Some(dev),
                "architecture_trace",
                arch,
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn node(&self, n: &NodeInput) -> Result<()> {
        self.node_bound(n, None)
    }
    pub fn node_bound(&self, n: &NodeInput, token: Option<&str>) -> Result<()> {
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        Self::node_tx(&tx, n, token)?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn node_tx(tx: &Transaction, n: &NodeInput, token: Option<&str>) -> Result<()> {
        if !matches!(
            n.kind.as_str(),
            "domain" | "module" | "submodule" | "function" | "work_package"
        ) || !matches!(
            n.plan_state.as_str(),
            "planned" | "active" | "blocked" | "paused" | "legacy"
        ) || n.id.is_empty()
            || n.title.is_empty()
            || n.generation < 0
        {
            return Err(Error::Invalid("node fields"));
        }
        if let Some(token) = token {
            crate::identity::authorize(tx, token, &n.project_id, Some(&n.id), "manager")?;
        }
        if !exists(tx, "map_projects", &n.project_id)? {
            return Err(Error::Missing);
        }
        if !exists_architecture(tx, &n.project_id)? {
            return Err(Error::Conflict("project architecture missing"));
        }
        if let Some(p) = &n.parent_id {
            let parent_project: Option<String> = tx
                .query_row(
                    "SELECT project_id FROM map_dev_nodes WHERE id=?",
                    [p],
                    |r| r.get(0),
                )
                .optional()?;
            if parent_project.as_deref() != Some(&n.project_id) {
                return Err(Error::Invalid("parent project"));
            }
            let parent_kind: String =
                tx.query_row("SELECT kind FROM map_dev_nodes WHERE id=?", [p], |r| {
                    r.get(0)
                })?;
            if parent_kind == "work_package" {
                return Err(Error::Invalid("work package is a leaf"));
            }
            let cycle: i64 = tx.query_row("WITH RECURSIVE a(id,parent_id) AS (SELECT id,parent_id FROM map_dev_nodes WHERE id=? UNION ALL SELECT p.id,p.parent_id FROM map_dev_nodes p JOIN a ON p.id=a.parent_id) SELECT EXISTS(SELECT 1 FROM a WHERE id=?)", params![p,n.id], |r| r.get(0))?;
            if cycle != 0 {
                return Err(Error::Invalid("parent cycle"));
            }
        }
        if n.kind == "work_package"
            && tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM map_dev_nodes WHERE parent_id=?)",
                [&n.id],
                |r| r.get::<_, bool>(0),
            )?
        {
            return Err(Error::Invalid("work package cannot have children"));
        }
        let old: Option<(String, Option<String>, i64)> = tx
            .query_row(
                "SELECT project_id,owner,generation FROM map_dev_nodes WHERE id=?",
                [&n.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((project, owner, g)) = old {
            if token.is_some() && (owner != n.owner || g != n.generation) {
                return Err(Error::Conflict("use compare-and-swap handoff"));
            }
            if project != n.project_id {
                return Err(Error::Conflict("node project immutable"));
            }
            if n.generation < g {
                return Err(Error::Conflict("generation regression"));
            }
            if n.generation == g && owner != n.owner {
                return Err(Error::Conflict("owner requires new generation"));
            }
        }
        let changed = tx.execute("INSERT INTO map_dev_nodes(id,project_id,parent_id,kind,title,goal,sort_key,plan_state,architecture_ref,owner,generation,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,unixepoch()) ON CONFLICT(id) DO UPDATE SET parent_id=excluded.parent_id,kind=excluded.kind,title=excluded.title,goal=excluded.goal,sort_key=excluded.sort_key,plan_state=excluded.plan_state,architecture_ref=excluded.architecture_ref,owner=excluded.owner,generation=excluded.generation,updated_at=excluded.updated_at WHERE map_dev_nodes.parent_id IS NOT excluded.parent_id OR map_dev_nodes.kind IS NOT excluded.kind OR map_dev_nodes.title IS NOT excluded.title OR map_dev_nodes.goal IS NOT excluded.goal OR map_dev_nodes.sort_key IS NOT excluded.sort_key OR map_dev_nodes.plan_state IS NOT excluded.plan_state OR map_dev_nodes.architecture_ref IS NOT excluded.architecture_ref OR map_dev_nodes.owner IS NOT excluded.owner OR map_dev_nodes.generation IS NOT excluded.generation", params![n.id,n.project_id,n.parent_id,n.kind,n.title,n.goal,n.sort_key,n.plan_state,n.architecture_ref,n.owner,n.generation])?;
        if changed > 0 {
            event(tx, &n.project_id, Some(&n.id), "node", "")?;
        }
        Ok(())
    }
    pub fn dependency(&self, from: &str, to: &str) -> Result<()> {
        if from == to {
            return Err(Error::Invalid("dependency cycle"));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let a: Option<String> = tx
            .query_row(
                "SELECT project_id FROM map_dev_nodes WHERE id=?",
                [from],
                |r| r.get(0),
            )
            .optional()?;
        let b: Option<String> = tx
            .query_row(
                "SELECT project_id FROM map_dev_nodes WHERE id=?",
                [to],
                |r| r.get(0),
            )
            .optional()?;
        if a.is_none() || b.is_none() {
            return Err(Error::Missing);
        }
        if a != b {
            return Err(Error::Invalid("dependency project boundary"));
        }
        let cycle: i64 = tx.query_row("WITH RECURSIVE d(id) AS (SELECT to_id FROM map_edges WHERE from_id=? AND kind='depends_on' UNION SELECT e.to_id FROM map_edges e JOIN d ON e.from_id=d.id WHERE e.kind='depends_on') SELECT EXISTS(SELECT 1 FROM d WHERE id=?)", params![to,from], |r| r.get(0))?;
        if cycle != 0 {
            return Err(Error::Invalid("dependency cycle"));
        }
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO map_edges(from_id,to_id,kind) VALUES(?,?,'depends_on')",
            params![from, to],
        )?;
        if inserted > 0 {
            event(&tx, a.as_deref().unwrap(), Some(from), "dependency", to)?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Trusted database-owner API; worker callers use authenticated submissions.
    pub fn checkpoint(&self, c: &CheckpointInput) -> Result<bool> {
        let input = crate::submission::Submission {
            base_checkpoint_rowid: None,
            submission_key: format!("checkpoint:{}:{}:{}", c.node_id, c.generation, c.version),
            checkpoint: c.clone(),
            artifacts: Vec::new(),
            resources: None,
        };
        Ok(self.submit(&input)?.created)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn accept(
        &self,
        node_id: &str,
        generation: i64,
        version: i64,
        digest: &str,
        decision: &str,
        evidence_level: &str,
        source_ref: &str,
    ) -> Result<()> {
        self.accept_bound(
            node_id,
            generation,
            version,
            digest,
            decision,
            evidence_level,
            source_ref,
            None,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn accept_bound(
        &self,
        node_id: &str,
        generation: i64,
        version: i64,
        digest: &str,
        decision: &str,
        evidence_level: &str,
        source_ref: &str,
        token: Option<&str>,
    ) -> Result<()> {
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        accept_tx(
            &tx,
            node_id,
            generation,
            version,
            digest,
            decision,
            evidence_level,
            source_ref,
            token,
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Trusted database-owner API; worker artifacts travel in their scoped submission.
    pub fn artifact(&self, a: &ArtifactInput) -> Result<()> {
        self.artifact_impl(a, None, false)
    }
    pub(crate) fn legacy_worker_artifact(&self, a: &ArtifactInput) -> Result<()> {
        self.artifact_impl(a, None, true)
    }
    pub fn artifact_checked(&self, a: &ArtifactInput, repo_root: &Path) -> Result<()> {
        if !repo_root.is_dir() || !crate::git_index::oid(&a.revision) {
            return Err(Error::Invalid("Git revision"));
        }
        let mut verified = a.clone();
        verified.verification = "verified".into();
        self.artifact_impl(&verified, Some(repo_root), false)
    }
    fn artifact_impl(
        &self,
        a: &ArtifactInput,
        checked: Option<&Path>,
        legacy_worker: bool,
    ) -> Result<()> {
        if checked.is_none() && a.verification == "verified" {
            return Err(Error::Invalid("verified requires Git check"));
        }
        if a.repo_key.is_empty()
            || a.revision.is_empty()
            || a.path.is_empty()
            || a.path.starts_with('/')
            || a.path.split('/').any(|p| p == "..")
            || !matches!(
                a.kind.as_str(),
                "repo"
                    | "commit"
                    | "directory"
                    | "file"
                    | "migration"
                    | "test"
                    | "evidence"
                    | "document"
            )
            || !matches!(a.provenance.as_str(), "declared" | "observed")
            || !matches!(a.verification.as_str(), "verified" | "unverified" | "draft")
        {
            return Err(Error::Invalid("artifact"));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let project: Option<String> = tx
            .query_row(
                "SELECT project_id FROM map_dev_nodes WHERE id=?",
                [&a.node_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(project) = project else {
            return Err(Error::Missing);
        };
        if legacy_worker {
            require_current_schema(&tx)?;
            let state = crate::takeover::global_worker_state(&tx, &project, &a.node_id)?;
            crate::takeover::worker_write_gate(&state, true)?;
        }
        if let Some(repo_root) = checked {
            let registered:String=tx.query_row("SELECT r.path FROM map_repositories r JOIN map_project_repositories p USING(repo_key) WHERE p.project_id=? AND r.repo_key=?",params![project,a.repo_key],|r|r.get(0)).optional()?.ok_or(Error::Invalid("repository registration"))?;
            if repo_root
                .canonicalize()
                .map_err(|_| Error::Invalid("repository root"))?
                != Path::new(&registered)
            {
                return Err(Error::Invalid("repository root mismatch"));
            }
            let target = if a.kind == "repo" || a.kind == "commit" {
                format!("{}^{{commit}}", a.revision)
            } else {
                format!("{}:{}", a.revision, a.path)
            };
            let ok = crate::git_index::immutable_git_command()
                .arg("-C")
                .arg(repo_root)
                .arg("cat-file")
                .arg("-e")
                .arg(&target)
                .output()
                .map(|x| x.status.success())
                .unwrap_or(false);
            if !ok {
                return Err(Error::Invalid("Git object or path missing"));
            }
            if let Some(expected) = &a.content_digest {
                verify_blob_digest(repo_root, &target, expected)?;
            }
        }
        let id = artifact_tx(&tx, a)?;
        event(&tx, &project, Some(&a.node_id), "artifact", &id.to_string())?;
        tx.commit()?;
        Ok(())
    }
    pub fn read<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T>,
    {
        let mut db = self.connect(true)?;
        let tx = db.transaction()?;
        require_current_schema(&tx)?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }
}
pub(crate) fn require_current_schema(tx: &Transaction) -> Result<()> {
    let version: i64 = if table_exists(tx, "map_schema_migrations")? {
        tx.query_row(
            "SELECT COALESCE(MAX(version),0) FROM map_schema_migrations",
            [],
            |r| r.get(0),
        )?
    } else {
        0
    };
    if version > crate::recovery::SCHEMA_VERSION {
        return Err(Error::Conflict("newer database schema; upgrade map tools"));
    }
    if version != crate::recovery::SCHEMA_VERSION {
        return Err(Error::Conflict(
            "run mapctl migrate before reading this database",
        ));
    }
    Ok(())
}
fn exists_architecture(tx: &Transaction<'_>, project: &str) -> rusqlite::Result<bool> {
    Ok(tx
        .query_row(
            "SELECT 1 FROM map_architectures WHERE project_id=?",
            [project],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}
fn table_exists(db: &Connection, name: &str) -> rusqlite::Result<bool> {
    Ok(db
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?",
            [name],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}
fn exists(tx: &Transaction<'_>, table: &str, id: &str) -> rusqlite::Result<bool> {
    let sql = match table {
        "map_projects" => "SELECT 1 FROM map_projects WHERE id=?",
        _ => "SELECT 1 FROM map_dev_nodes WHERE id=?",
    };
    Ok(tx
        .query_row(sql, [id], |r| r.get::<_, i64>(0))
        .optional()?
        .is_some())
}
pub(crate) fn event(
    tx: &Transaction<'_>,
    project: &str,
    node: Option<&str>,
    kind: &str,
    detail: &str,
) -> Result<()> {
    tx.execute("INSERT INTO map_events(project_id,node_id,kind,detail,created_at) VALUES(?,?,?,?,unixepoch())",params![project,node,kind,detail])?;
    crate::navigation::changed(tx, project, node, kind)?;
    Ok(())
}

pub fn limit(raw: Option<usize>) -> usize {
    raw.unwrap_or(50).clamp(1, MAX_LIMIT)
}
pub fn list_projects(tx: &Transaction<'_>, after: &str, n: usize) -> Result<Value> {
    let mut s=tx.prepare("SELECT p.id,p.name,p.source_ref,p.updated_at,(SELECT COUNT(*) FROM map_dev_nodes n WHERE n.project_id=p.id) FROM map_projects p WHERE p.id>? ORDER BY p.id LIMIT ?")?;
    let rows=s.query_map(params![after,(n+1) as i64],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"source_ref":r.get::<_,String>(2)?,"updated_at":r.get::<_,f64>(3)?,"node_count":r.get::<_,i64>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(rows, n, "id"))
}
pub fn children(
    tx: &Transaction<'_>,
    parent: &str,
    after: &str,
    n: usize,
    state: Option<&str>,
) -> Result<Value> {
    let (project, parent_id) = if let Some(p) = parent.strip_prefix("project:") {
        (p.to_string(), None)
    } else {
        let p: String = tx
            .query_row(
                "SELECT project_id FROM map_dev_nodes WHERE id=?",
                [parent],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Missing)?;
        (p, Some(parent))
    };
    let mut s=tx.prepare("SELECT id,title,kind,plan_state,sort_key FROM map_dev_nodes WHERE project_id=? AND parent_id IS ? AND (sort_key||char(31)||id)>? AND (? IS NULL OR plan_state=?) ORDER BY sort_key,id LIMIT ?")?;
    let rows=s.query_map(params![project,parent_id,after,state,state,(n+1) as i64],|r|{let id:String=r.get(0)?;let key:String=r.get(4)?;Ok(json!({"id":id,"title":r.get::<_,String>(1)?,"kind":r.get::<_,String>(2)?,"plan_state":r.get::<_,String>(3)?,"cursor":format!("{key}\u{1f}{id}")}))})?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(rows, n, "cursor"))
}
pub fn children_of_node(
    tx: &Transaction<'_>,
    node: &str,
    after: &str,
    n: usize,
    state: Option<&str>,
) -> Result<Value> {
    let project: Option<String> = tx
        .query_row(
            "SELECT project_id FROM map_dev_nodes WHERE id=?",
            [node],
            |r| r.get(0),
        )
        .optional()?;
    let Some(project) = project else {
        return Err(Error::Missing);
    };
    let n = limit(Some(n));
    let mut s=tx.prepare("SELECT id,title,kind,plan_state,sort_key FROM map_dev_nodes WHERE project_id=? AND parent_id IS ? AND (sort_key||char(31)||id)>? AND (? IS NULL OR plan_state=?) ORDER BY sort_key,id LIMIT ?")?;
    let rows=s.query_map(params![project,Some(node),after,state,state,(n+1) as i64],|r|{let id:String=r.get(0)?;let key:String=r.get(4)?;Ok(json!({"id":id,"title":r.get::<_,String>(1)?,"kind":r.get::<_,String>(2)?,"plan_state":r.get::<_,String>(3)?,"cursor":format!("{key}\u{1f}{id}")}))})?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(rows, n, "cursor"))
}
pub fn node_detail(tx: &Transaction<'_>, id: &str) -> Result<Value> {
    let mut s=tx.prepare("SELECT id,project_id,parent_id,kind,title,goal,plan_state,architecture_ref,owner,generation FROM map_dev_nodes WHERE id=?")?;
    let v:Option<Value>=s.query_row([id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"project_id":r.get::<_,String>(1)?,"parent_id":r.get::<_,Option<String>>(2)?,"kind":r.get::<_,String>(3)?,"title":r.get::<_,String>(4)?,"goal":r.get::<_,String>(5)?,"plan_state":r.get::<_,String>(6)?,"architecture_ref":r.get::<_,Option<String>>(7)?,"owner":r.get::<_,Option<String>>(8)?,"generation":r.get::<_,i64>(9)?}))).optional()?;
    let mut v = v.ok_or(Error::Missing)?;
    let latest:Option<(i64,i64,String,String,String)>=tx.query_row("SELECT generation,version,digest,summary,arch_version FROM map_checkpoint_index WHERE node_id=? ORDER BY generation DESC,version DESC LIMIT 1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let accepted:Option<(i64,i64,String,String)>=tx.query_row("SELECT generation,version,digest,evidence_level FROM map_acceptances WHERE node_id=? AND decision='accepted' ORDER BY generation DESC,version DESC LIMIT 1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    v["development_state"] = json!(if latest.is_some() {
        "submitted"
    } else {
        "none"
    });
    let current_arch: String = tx
        .query_row(
            "SELECT version FROM map_architectures WHERE project_id=?",
            [v["project_id"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap_or_default();
    v["acceptance_state"] = json!(match (&latest, &accepted) {
        (Some(l), Some(a)) if l.0 == a.0 && l.1 == a.1 && l.2 == a.2 && l.4 == current_arch =>
            "accepted",
        (_, Some(_)) => "stale",
        _ => "pending",
    });
    v["latest_checkpoint"] = json!(latest.map(
        |x| json!({"generation":x.0,"version":x.1,"digest":x.2,"summary":x.3,"arch_version":x.4})
    ));
    v["accepted"] =
        json!(accepted.map(|x| json!({"generation":x.0,"version":x.1,"evidence_level":x.3})));
    let mut d =
        tx.prepare("SELECT e.to_id FROM map_edges e JOIN map_dev_nodes a ON a.id=e.from_id JOIN map_dev_nodes b ON b.id=e.to_id WHERE e.from_id=? AND e.kind='depends_on' AND a.project_id=b.project_id LIMIT 50")?;
    v["depends_on"] = json!(d
        .query_map([id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?);
    let project = v["project_id"].as_str().unwrap();
    let architecture: Option<(String, String, String)> = tx
        .query_row(
            "SELECT version,source_ref,digest FROM map_architectures WHERE project_id=?",
            [project],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    v["architecture"] =
        json!(architecture.map(|x| json!({"version":x.0,"source_ref":x.1,"digest":x.2})));
    let mut t=tx.prepare("SELECT a.id,a.title,t.relation,t.source_ref FROM map_local_traces t JOIN map_arch_nodes a ON a.id=t.arch_node_id AND a.arch_version=t.arch_version WHERE t.dev_node_id=? AND a.arch_version=(SELECT version FROM map_architectures WHERE project_id=a.project_id) LIMIT 50")?;
    v["architecture_traces"]=json!(t.query_map([id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"relation":r.get::<_,String>(2)?,"source_ref":r.get::<_,String>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?);
    let project = v["project_id"].as_str().unwrap().to_string();
    let nav = crate::navigation::focus(tx, &project, None, &format!("dev:{id}"))?;
    v["acceptance_state"] = nav["node"]["acceptance_state"].clone();
    v["development_state"] = nav["node"]["development_state"].clone();
    v["dependencies_page"] = crate::relations::dependencies(tx, id, "", 50)?;
    v["architecture_traces_page"] =
        crate::relations::node_traces(tx, id, nav["revision"].as_str().unwrap_or(""), "", 50)?;
    v["navigation"] = nav;
    v["local_source"] = json!(crate::local_source::node(tx, id)?);
    Ok(v)
}
pub fn checkpoints(tx: &Transaction<'_>, id: &str, after: i64, n: usize) -> Result<Value> {
    // Read-only receipt history: a new session token bound to the same project
    // (and node, for workers) can recover it without the original submitter token.
    let mut s=tx.prepare("SELECT c.rowid,c.node_id,c.owner,c.generation,c.version,c.digest,c.summary,c.source_ref,c.git_revision,c.created_at,s.submission_key FROM map_checkpoint_index c LEFT JOIN map_submissions s ON s.checkpoint_rowid=c.id WHERE c.node_id=? AND c.rowid>? ORDER BY c.rowid LIMIT ?")?;
    let rows=s.query_map(params![id,after,(n+1) as i64],|r|Ok(json!({"cursor":r.get::<_,i64>(0)?,"node_id":r.get::<_,String>(1)?,"owner":r.get::<_,String>(2)?,"generation":r.get::<_,i64>(3)?,"version":r.get::<_,i64>(4)?,"digest":r.get::<_,String>(5)?,"summary":r.get::<_,String>(6)?,"source_ref":r.get::<_,String>(7)?,"git_revision":r.get::<_,Option<String>>(8)?,"created_at":r.get::<_,f64>(9)?,"submission_key":r.get::<_,Option<String>>(10)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(rows, n, "cursor"))
}
pub fn artifacts(
    tx: &Transaction<'_>,
    id: &str,
    after: i64,
    n: usize,
    kind: Option<&str>,
) -> Result<Value> {
    let mut s=tx.prepare("SELECT a.id,a.repo_key,a.revision,a.path,a.kind,a.verification,a.content_digest,na.provenance FROM map_local_node_artifacts na JOIN map_artifacts a ON a.id=na.artifact_id WHERE na.node_id=? AND a.id>? AND (? IS NULL OR a.kind=?) ORDER BY a.id LIMIT ?")?;
    let rows=s.query_map(params![id,after,kind,kind,(n+1) as i64],|r|Ok(json!({"cursor":r.get::<_,i64>(0)?,"repo_key":r.get::<_,String>(1)?,"revision":r.get::<_,String>(2)?,"path":r.get::<_,String>(3)?,"kind":r.get::<_,String>(4)?,"verification":r.get::<_,String>(5)?,"content_digest":r.get::<_,Option<String>>(6)?,"provenance":r.get::<_,String>(7)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(rows, n, "cursor"))
}
pub fn lookup(tx: &Transaction<'_>, repo: &str, rev: &str, path: &str, n: usize) -> Result<Value> {
    lookup_page(tx, repo, rev, path, 0, "", n)
}
pub fn lookup_page(
    tx: &Transaction<'_>,
    repo: &str,
    rev: &str,
    path: &str,
    after_artifact: i64,
    after_node: &str,
    n: usize,
) -> Result<Value> {
    let mut s=tx.prepare("SELECT a.id,n.id,n.title,a.path,a.kind,na.provenance FROM map_artifacts a JOIN map_local_node_artifacts na ON na.artifact_id=a.id JOIN map_dev_nodes n ON n.id=na.node_id WHERE a.repo_key=? AND a.revision=? AND (a.path=? OR (a.kind='directory' AND substr(?,1,length(a.path)+1)=a.path||'/')) AND (a.id>? OR (a.id=? AND n.id>?)) ORDER BY a.id,n.id LIMIT ?")?;
    let rows=s.query_map(params![repo,rev,path,path,after_artifact,after_artifact,after_node,(n+1) as i64],|r|{let aid:i64=r.get(0)?;let node:String=r.get(1)?;Ok(json!({"cursor":format!("{aid}:{node}"),"node_id":node,"title":r.get::<_,String>(2)?,"matched_path":r.get::<_,String>(3)?,"kind":r.get::<_,String>(4)?,"provenance":r.get::<_,String>(5)?}))})?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(rows, n, "cursor"))
}
pub fn changes(tx: &Transaction<'_>, project: &str, after: i64, n: usize) -> Result<Value> {
    let mut s=tx.prepare("SELECT id,node_id,kind,detail,created_at FROM map_events WHERE project_id=? AND id>? ORDER BY id LIMIT ?")?;
    let rows=s.query_map(params![project,after,(n+1) as i64],|r|Ok(json!({"cursor":r.get::<_,i64>(0)?,"node_id":r.get::<_,Option<String>>(1)?,"kind":r.get::<_,String>(2)?,"detail":r.get::<_,String>(3)?,"created_at":r.get::<_,f64>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let last = rows
        .iter()
        .take(n)
        .next_back()
        .and_then(|v| v["cursor"].as_i64())
        .unwrap_or(after);
    let mut out = page(rows, n, "cursor");
    out["last_event_id"] = json!(last);
    Ok(out)
}
fn page(mut rows: Vec<Value>, n: usize, key: &str) -> Value {
    let more = rows.len() > n;
    rows.truncate(n);
    let next = if more {
        rows.last().and_then(|x| x.get(key)).cloned()
    } else {
        None
    };
    json!({"items":rows,"next":next})
}

pub fn architecture(tx: &Transaction<'_>, project: &str) -> Result<Value> {
    architecture_revision(tx, project, None)
}
pub fn architecture_revision(
    tx: &Transaction<'_>,
    project: &str,
    revision: Option<&str>,
) -> Result<Value> {
    let current: Option<String> = tx
        .query_row(
            "SELECT version FROM map_architectures WHERE project_id=?",
            [project],
            |r| r.get(0),
        )
        .optional()?;
    let Some(version) = revision.or(current.as_deref()) else {
        return Ok(json!({"status":"unadopted"}));
    };
    let mut v:Value=tx.query_row("SELECT version,source_ref,digest,variant,adoption_ref,adopted_at FROM map_architecture_revisions WHERE project_id=? AND version=?",params![project,version],|r|Ok(json!({"version":r.get::<_,String>(0)?,"source_ref":r.get::<_,String>(1)?,"digest":r.get::<_,String>(2)?,"variant":r.get::<_,String>(3)?,"adoption_ref":r.get::<_,String>(4)?,"adopted_at":r.get::<_,f64>(5)?}))).optional()?.ok_or(Error::Missing)?;
    let (components,uncovered):(i64,i64)=tx.query_row("SELECT COUNT(*),COALESCE(SUM(CASE WHEN a.required=1 AND NOT EXISTS(SELECT 1 FROM map_local_traces t WHERE t.arch_node_id=a.id AND t.arch_version=a.arch_version) THEN 1 ELSE 0 END),0) FROM map_arch_nodes a WHERE a.project_id=? AND a.arch_version=?",params![project,version],|r|Ok((r.get(0)?,r.get(1)?)))?;
    v["current"] = json!(current.as_deref() == Some(version));
    v["component_count"] = json!(components);
    v["uncovered_count"] = json!(uncovered);
    Ok(v)
}
pub fn architecture_children(
    tx: &Transaction<'_>,
    project: &str,
    parent: Option<&str>,
    after: &str,
    n: usize,
) -> Result<Value> {
    architecture_children_revision(tx, project, parent, after, n, None)
}
/// Resolve the owning project of an architecture node.  The table key is
/// `(id, arch_version)`, so an id can legitimately appear in more than one
/// project/revision; a bare id is therefore ambiguous and must not be resolved
/// with an arbitrary `LIMIT 1`.  Callers that supply a revision get the exact
/// owner; without one, only globally unique ids resolve.
pub fn arch_project(tx: &Transaction<'_>, id: &str, revision: Option<&str>) -> Result<String> {
    if let Some(rev) = revision {
        return tx
            .query_row(
                "SELECT project_id FROM map_arch_nodes WHERE id=? AND arch_version=?",
                params![id, rev],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Missing);
    }
    let mut stmt = tx.prepare("SELECT DISTINCT project_id FROM map_arch_nodes WHERE id=?")?;
    let projects = stmt
        .query_map([id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    match projects.as_slice() {
        [project] => Ok(project.clone()),
        _ => Err(Error::Missing),
    }
}

pub fn architecture_children_revision(
    tx: &Transaction<'_>,
    project: &str,
    parent: Option<&str>,
    after: &str,
    n: usize,
    revision: Option<&str>,
) -> Result<Value> {
    let mut s=tx.prepare("SELECT id,title,kind,source_ref,sort_key FROM map_arch_nodes WHERE project_id=? AND arch_version=COALESCE(?,(SELECT version FROM map_architectures WHERE project_id=?)) AND parent_id IS ? AND (sort_key||char(31)||id)>? ORDER BY sort_key,id LIMIT ?")?;
    let rows=s.query_map(params![project,revision,project,parent,after,(n+1) as i64],|r|{let id:String=r.get(0)?;let key:String=r.get(4)?;Ok(json!({"id":id,"title":r.get::<_,String>(1)?,"kind":r.get::<_,String>(2)?,"source_ref":r.get::<_,String>(3)?,"cursor":format!("{key}\u{1f}{id}")}))})?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(page(rows, n, "cursor"))
}
pub fn architecture_trace(tx: &Transaction<'_>, id: &str, n: usize) -> Result<Value> {
    architecture_trace_revision(tx, id, n, None)
}
pub fn architecture_trace_revision(
    tx: &Transaction<'_>,
    id: &str,
    n: usize,
    revision: Option<&str>,
) -> Result<Value> {
    arch_project(tx, id, revision)?;
    let mut s=tx.prepare("SELECT n.id,n.title,t.relation,t.source_ref FROM map_local_traces t JOIN map_dev_nodes n ON n.id=t.dev_node_id JOIN map_arch_nodes a ON a.id=t.arch_node_id AND a.arch_version=t.arch_version WHERE t.arch_node_id=? AND a.arch_version=COALESCE(?,(SELECT version FROM map_architectures WHERE project_id=a.project_id)) ORDER BY n.id LIMIT ?")?;
    let rows=s.query_map(params![id,revision,n as i64],|r|Ok(json!({"node_id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"relation":r.get::<_,String>(2)?,"source_ref":r.get::<_,String>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({"items":rows}))
}

pub fn navigation_children(
    tx: &Transaction<'_>,
    parent: &str,
    after: &str,
    n: usize,
) -> Result<Value> {
    let project:Option<(String,String)>=tx.query_row("SELECT n.project_id,n.revision FROM map_navigation n LEFT JOIN map_architectures a ON a.project_id=n.project_id WHERE n.node_ref=? AND n.revision=COALESCE(a.version,'') LIMIT 1",[parent],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let Some((project, rev)) = project else {
        return Err(Error::Missing);
    };
    crate::navigation::children(tx, &project, Some(&rev), parent, after, n, None)
}
pub fn architecture_neighborhood(tx: &Transaction<'_>, id: &str, n: usize) -> Result<Value> {
    architecture_neighborhood_revision(tx, id, n, None)
}
pub fn architecture_neighborhood_revision(
    tx: &Transaction<'_>,
    id: &str,
    n: usize,
    revision: Option<&str>,
) -> Result<Value> {
    arch_project(tx, id, revision)?;
    let node:Option<(String,String,String,String,i64,Option<String>)>=tx.query_row("SELECT a.project_id,a.arch_version,a.title,a.source_ref,a.required,a.requirement_id FROM map_arch_nodes a JOIN map_architectures b ON b.project_id=a.project_id WHERE a.id=? AND a.arch_version=COALESCE(?,b.version)",params![id,revision],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
    let Some((project, version, title, source, required, requirement_id)) = node else {
        return Err(Error::Missing);
    };
    let children = architecture_children_revision(tx, &project, Some(id), "", n, revision)?;
    let traces = architecture_trace_revision(tx, id, n, revision)?;
    let mut stmt = tx.prepare(
        "SELECT to_id,kind,source_ref FROM map_local_interfaces WHERE from_id=? AND arch_version=COALESCE(?,(SELECT version FROM map_architectures WHERE project_id=?)) LIMIT ?",
    )?;
    let edges=stmt.query_map(params![id,revision,project,n as i64],|r|Ok(json!({"to_id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"source_ref":r.get::<_,String>(2)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(
        json!({"id":id,"project_id":project,"version":version,"title":title,"source_ref":source,"requirement_id":requirement_id,"required":required!=0,"children":children,"traces":traces,"interfaces":edges,"uncovered":required!=0 && traces["items"].as_array().is_some_and(|a|a.is_empty())}),
    )
}
pub fn search(tx: &Transaction<'_>, project: &str, q: &str, n: usize) -> Result<Value> {
    if q.len() < 2 || q.len() > 100 {
        return Err(Error::Invalid("search query"));
    }
    let pattern = format!("{}%", q.replace('%', "\\%").replace('_', "\\_"));
    let mut s=tx.prepare("SELECT id,title,kind,plan_state FROM map_dev_nodes WHERE project_id=? AND (id LIKE ? ESCAPE '\\' OR title LIKE ? ESCAPE '\\') ORDER BY title,id LIMIT ?")?;
    let rows=s.query_map(params![project,pattern,pattern,n as i64],|r|Ok(json!({"id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"kind":r.get::<_,String>(2)?,"plan_state":r.get::<_,String>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({"items":rows}))
}

// SHA-256 of the exact Git blob bytes, independent of Git's object ID.
fn verify_blob_digest(root: &Path, target: &str, expected: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    use std::process::Stdio;
    let mut child = crate::git_index::immutable_git_command()
        .arg("-C")
        .arg(root)
        .args(["cat-file", "blob", target])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Error::Invalid("cannot read Git blob"))?;
    let mut hash = Sha256::new();
    let copied = std::io::copy(&mut child.stdout.take().unwrap(), &mut hash);
    let status = child
        .wait()
        .map_err(|_| Error::Invalid("cannot finish Git blob read"))?;
    if copied.is_err() || !status.success() || format!("{:x}", hash.finalize()) != expected {
        return Err(Error::Invalid(
            "digest must match the SHA-256 of a Git blob",
        ));
    }
    Ok(())
}

pub(crate) fn artifact_tx(tx: &Transaction<'_>, a: &ArtifactInput) -> Result<i64> {
    if a.repo_key.is_empty()
        || a.revision.is_empty()
        || a.path.is_empty()
        || a.path.starts_with('/')
        || a.path.split('/').any(|p| p == "..")
        || !matches!(
            a.kind.as_str(),
            "repo"
                | "commit"
                | "directory"
                | "file"
                | "migration"
                | "test"
                | "evidence"
                | "document"
        )
        || !matches!(a.provenance.as_str(), "declared" | "observed")
        || !matches!(a.verification.as_str(), "verified" | "unverified" | "draft")
    {
        return Err(Error::Invalid("artifact"));
    }
    let project:String=tx.query_row("SELECT n.project_id FROM map_dev_nodes n JOIN map_project_repositories r ON r.project_id=n.project_id WHERE n.id=? AND r.repo_key=?",params![a.node_id,a.repo_key],|r|r.get(0)).optional()?.ok_or(Error::Invalid("repository is not registered to this project"))?;
    let existing: Option<(String, Option<String>)> = tx.query_row(
        "SELECT verification,content_digest FROM map_artifacts WHERE project_id=? AND repo_key=? AND revision=? AND path=? AND kind=?",
        params![project,a.repo_key,a.revision,a.path,a.kind],
        |row| Ok((row.get(0)?,row.get(1)?)),
    ).optional()?;
    let verified = a.verification == "verified";
    let (status, digest) = match existing.as_ref() {
        None => (a.verification.as_str(), a.content_digest.as_ref()),
        Some((old_status, old_digest)) => {
            if !verified {
                if let Some(claim) = &a.content_digest {
                    if (old_status == "verified" || old_digest.is_some())
                        && old_digest.as_ref() != Some(claim)
                    {
                        return Err(Error::Conflict(
                            "artifact digest conflicts with existing metadata",
                        ));
                    }
                }
                (
                    old_status.as_str(),
                    old_digest.as_ref().or(a.content_digest.as_ref()),
                )
            } else if old_status == "verified" {
                (
                    "verified",
                    a.content_digest.as_ref().or(old_digest.as_ref()),
                )
            } else {
                // An existence-only check cannot promote an earlier declared digest.
                ("verified", a.content_digest.as_ref())
            }
        }
    };
    tx.execute("INSERT INTO map_artifacts(project_id,repo_key,revision,path,kind,verification,content_digest)
                VALUES(?,?,?,?,?,?,?) ON CONFLICT(project_id,repo_key,revision,path,kind)
                DO UPDATE SET verification=excluded.verification,content_digest=excluded.content_digest",
                params![project,a.repo_key,a.revision,a.path,a.kind,status,digest])?;
    let id: i64 = tx.query_row(
        "SELECT id FROM map_artifacts WHERE project_id=? AND repo_key=? AND revision=? AND path=? AND kind=?",
        params![project,a.repo_key, a.revision, a.path, a.kind],
        |r| r.get(0),
    )?;
    tx.execute("INSERT INTO map_node_artifacts(node_id,artifact_id,provenance) VALUES(?,?,?) ON CONFLICT(node_id,artifact_id) DO UPDATE SET provenance=CASE WHEN map_node_artifacts.provenance='declared' THEN 'declared' ELSE excluded.provenance END",params![a.node_id,id,a.provenance])?;
    Ok(id)
}

pub(crate) fn accept_tx(
    tx: &Transaction<'_>,
    node_id: &str,
    generation: i64,
    version: i64,
    digest: &str,
    decision: &str,
    evidence_level: &str,
    source_ref: &str,
    token: Option<&str>,
) -> Result<()> {
    if !matches!(decision, "accepted" | "rejected")
        || !matches!(
            evidence_level,
            "document" | "host" | "simulation" | "board" | "product"
        )
        || source_ref.is_empty()
    {
        return Err(Error::Invalid("acceptance"));
    }
    let found:Option<(String,String)> = tx.query_row("SELECT n.project_id,c.digest FROM map_dev_nodes n JOIN map_checkpoint_index c ON c.node_id=n.id WHERE n.id=? AND c.generation=? AND c.version=? AND n.generation=c.generation AND n.owner=c.owner AND c.arch_version=(SELECT version FROM map_architectures WHERE project_id=n.project_id)",params![node_id,generation,version],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let Some((project, actual)) = found else {
        return Err(Error::Missing);
    };
    if let Some(token) = token {
        crate::identity::authorize(tx, token, &project, Some(node_id), "manager")?;
    }
    let current:Option<(i64,i64)>=tx.query_row("SELECT generation,version FROM map_checkpoint_index WHERE node_id=? ORDER BY generation DESC,version DESC LIMIT 1",[node_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if current != Some((generation, version)) {
        return Err(Error::Conflict("checkpoint no longer latest"));
    }
    if actual != digest {
        return Err(Error::Conflict("checkpoint digest"));
    }
    if decision == "accepted" {
        crate::resources::acceptance_gate(tx, node_id)?;
    }
    tx.execute("INSERT INTO map_acceptances(node_id,generation,version,digest,decision,evidence_level,source_ref,created_at) VALUES(?,?,?,?,?,?,?,unixepoch()) ON CONFLICT(node_id,generation,version) DO UPDATE SET decision=excluded.decision,evidence_level=excluded.evidence_level,source_ref=excluded.source_ref",params![node_id,generation,version,digest,decision,evidence_level,source_ref])?;
    event(tx, &project, Some(node_id), "acceptance", decision)?;
    Ok(())
}
