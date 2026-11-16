// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
use crate::{
    navigation,
    store::{Error, MapStore, Result},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub const SCHEMA_VERSION: i64 = 12;
pub fn backup(store: &MapStore, destination: &Path) -> Result<()> {
    if destination.exists() {
        return Err(Error::Conflict("backup destination exists"));
    }
    let source = store.connect(true)?;
    let mut target = Connection::open(destination)?;
    let job = rusqlite::backup::Backup::new(&source, &mut target)?;
    job.run_to_completion(256, Duration::from_millis(5), None)?;
    drop(job);
    integrity(&target)?;
    Ok(())
}
pub fn restore(source: &Path, destination: &Path) -> Result<MapStore> {
    if destination.exists() {
        return Err(Error::Conflict("restore destination exists"));
    }
    let store = MapStore::new(source);
    integrity(&store.connect(true)?)?;
    backup(&store, destination)?;
    Ok(MapStore::new(destination))
}
pub fn integrity(db: &Connection) -> Result<()> {
    let status: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if status != "ok" {
        return Err(Error::Conflict("SQLite integrity check failed"));
    }
    let foreign_key_problem = db.prepare("PRAGMA foreign_key_check")?.exists([])?;
    if foreign_key_problem {
        return Err(Error::Conflict("SQLite foreign key check failed"));
    }
    Ok(())
}
fn table(db: &Connection, name: &str) -> Result<bool> {
    Ok(db
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?",
            [name],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}
fn install_legacy_worker_snapshot(tx: &rusqlite::Transaction, prior_version: i64) -> Result<()> {
    tx.execute_batch(include_str!("legacy_workers.sql"))?;
    // V11 mixed new and migrated assignments without recording their provenance.
    if (1..=10).contains(&prior_version) {
        tx.execute_batch(
            "INSERT INTO map_rc2_nodes
            SELECT id,owner,generation FROM map_dev_nodes WHERE owner IS NOT NULL
            AND NOT EXISTS(SELECT 1 FROM map_local_nodes WHERE node_id=map_dev_nodes.id);
            INSERT INTO map_rc2_workers SELECT token_hash FROM map_identities
            WHERE revoked=0 AND json_extract(binding,'$.role')='worker'
            AND json_extract(binding,'$.expires_at')>unixepoch();",
        )?;
    }
    Ok(())
}
pub fn migrate(store: &MapStore) -> Result<()> {
    let mut db = store.connect(false)?;
    let version = if table(&db, "map_schema_migrations")? {
        db.query_row(
            "SELECT COALESCE(MAX(version),0) FROM map_schema_migrations",
            [],
            |r| r.get(0),
        )?
    } else {
        0
    };
    if version > SCHEMA_VERSION {
        return Err(Error::Conflict("map schema is newer than this component"));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%')",[],|r|r.get::<_,bool>(0))? {
        let target = PathBuf::from(format!(
            "{}.before-map-v{}.{}.sqlite",
            store.path.display(),
            SCHEMA_VERSION,
            uuid::Uuid::new_v4()
        ));
        backup(store, &target)?;
    }
    db.pragma_update(None, "journal_mode", "WAL")?;
    db.pragma_update(None, "foreign_keys", "OFF")?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Another migrator may have completed while we were backing up.
    if table(&tx, "map_schema_migrations")? {
        let current: i64 = tx.query_row(
            "SELECT COALESCE(MAX(version),0) FROM map_schema_migrations",
            [],
            |r| r.get(0),
        )?;
        if current == SCHEMA_VERSION {
            return Ok(());
        }
        if matches!(current, 10 | 11) {
            tx.execute_batch(include_str!("takeover.sql"))?;
            install_legacy_worker_snapshot(&tx, current)?;
            tx.execute(
                "INSERT INTO map_schema_migrations VALUES(?,unixepoch())",
                [SCHEMA_VERSION],
            )?;
            integrity(&tx)?;
            tx.commit()?;
            return Ok(());
        }
        if current > SCHEMA_VERSION {
            return Err(Error::Conflict("map schema is newer than this component"));
        }
    }
    // Early isolated v2 used an ID-only architecture key. Preserve its rows during upgrade.
    let old_arch = if table(&tx, "map_arch_traces")? {
        let mut q = tx.prepare("PRAGMA table_info(map_arch_traces)")?;
        let columns = q
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        !columns.iter().any(|v| v == "arch_version")
    } else {
        false
    };
    if old_arch {
        tx.execute_batch("DROP VIEW IF EXISTS map_nodes; DROP INDEX IF EXISTS map_arch_nodes_parent; DROP INDEX IF EXISTS map_arch_traces_dev; DROP INDEX IF EXISTS map_arch_traces_arch; DROP INDEX IF EXISTS map_architecture_edges_reverse; ALTER TABLE map_arch_nodes RENAME TO map_old_arch_nodes; ALTER TABLE map_arch_traces RENAME TO map_old_arch_traces;")?;
        if table(&tx, "map_architecture_edges")? {
            tx.execute_batch("ALTER TABLE map_architecture_edges RENAME TO map_old_arch_edges;")?;
        }
    }
    let old_checkpoint = if table(&tx, "map_checkpoint_index")? {
        let mut q = tx.prepare("PRAGMA table_info(map_checkpoint_index)")?;
        let cols = q
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        !cols.iter().any(|c| c == "id")
    } else {
        false
    };
    if old_checkpoint {
        tx.execute_batch("DROP INDEX IF EXISTS map_checkpoint_node;DROP INDEX IF EXISTS map_checkpoint_page;ALTER TABLE map_checkpoint_index RENAME TO map_old_checkpoint_index;")?;
    }
    // Keep legacy IDs and links; rows without one unambiguous authorized owner remain quarantined.
    let old_art = table(&tx,"map_artifacts")? && !tx.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('map_artifacts') WHERE name='project_id')",[],|r|r.get::<_,bool>(0))?;
    if old_art {
        tx.execute_batch("CREATE TEMP TABLE legacy_art AS SELECT * FROM map_artifacts; DROP TABLE map_artifacts;")?;
    }
    tx.execute_batch("DROP VIEW IF EXISTS map_nodes")?;
    tx.execute_batch(include_str!("schema.sql"))?;
    if old_checkpoint {
        tx.execute_batch("INSERT INTO map_checkpoint_index SELECT rowid,* FROM map_old_checkpoint_index;DROP TABLE map_old_checkpoint_index;")?;
    }

    let legacy_keys = table(&tx, "map_submissions")? && tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('map_submissions') WHERE name='node_id' AND pk=0)",
        [], |row| row.get::<_, bool>(0),
    )?;
    if legacy_keys {
        tx.execute_batch("ALTER TABLE map_submissions RENAME TO map_v7_submissions")?;
    }
    tx.execute_batch(include_str!("upgrade.sql"))?;
    if old_art {
        tx.execute_batch("WITH owners AS (SELECT l.artifact_id,n.project_id FROM map_node_artifacts l JOIN map_dev_nodes n ON n.id=l.node_id UNION SELECT c.artifact_id,n.project_id FROM map_checkpoint_artifacts c JOIN map_checkpoint_index i ON i.id=c.checkpoint_rowid JOIN map_dev_nodes n ON n.id=i.node_id), single_owner AS (SELECT artifact_id,min(project_id) AS project_id FROM owners GROUP BY artifact_id HAVING count(*)=1) INSERT INTO map_artifacts(id,project_id,repo_key,revision,path,kind,verification,content_digest) SELECT a.id,r.project_id,a.repo_key,a.revision,a.path,a.kind,a.verification,a.content_digest FROM legacy_art a LEFT JOIN single_owner o ON o.artifact_id=a.id LEFT JOIN map_project_repositories r ON (r.project_id,r.repo_key)=(o.project_id,a.repo_key); DROP TABLE legacy_art;")?;
    }
    // A pre-v10 verified flag cannot establish which Git object was checked.
    // Archive the exact claim before removing its authority from live reads.
    tx.execute_batch(
        "INSERT OR IGNORE INTO map_legacy_artifact_verifications
        (artifact_id,verification,content_digest)
        SELECT id,verification,content_digest FROM map_artifacts WHERE verification='verified';
        UPDATE map_artifacts SET verification='unverified',content_digest=NULL
        WHERE verification='verified';",
    )?;
    if legacy_keys {
        tx.execute_batch("INSERT INTO map_submissions SELECT * FROM map_v7_submissions; DROP TABLE map_v7_submissions")?;
    }
    if old_arch {
        tx.execute_batch("INSERT INTO map_arch_nodes(id,project_id,arch_version,parent_id,title,kind,source_ref,sort_key,requirement_id,required) SELECT id,project_id,arch_version,parent_id,title,kind,source_ref,sort_key,requirement_id,required FROM map_old_arch_nodes; INSERT INTO map_arch_traces SELECT t.arch_node_id,a.arch_version,t.dev_node_id,t.relation,t.source_ref FROM map_old_arch_traces t JOIN map_old_arch_nodes a ON a.id=t.arch_node_id;")?;
        if table(&tx, "map_old_arch_edges")? {
            tx.execute_batch("INSERT INTO map_architecture_edges SELECT e.from_id,e.to_id,a.arch_version,e.kind,e.source_ref FROM map_old_arch_edges e JOIN map_old_arch_nodes a ON a.id=e.from_id; DROP TABLE map_old_arch_edges;")?;
        }
        tx.execute_batch("DROP TABLE map_old_arch_traces; DROP TABLE map_old_arch_nodes;")?;
    }
    tx.execute("INSERT OR IGNORE INTO map_architecture_revisions(project_id,version,source_ref,digest,variant,adoption_ref,adopted_at) SELECT project_id,version,source_ref,digest,'legacy',source_ref,adopted_at FROM map_architectures",[])?;
    // Preserve checkpoint-only retry semantics across the legacy migration.
    {
        use sha2::{Digest, Sha256};
        let mut q=tx.prepare("SELECT id,node_id,owner,generation,version,digest,summary,source_ref,git_revision FROM map_checkpoint_index WHERE id NOT IN (SELECT checkpoint_rowid FROM map_submissions)")?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    crate::store::CheckpointInput {
                        node_id: r.get(1)?,
                        owner: r.get(2)?,
                        generation: r.get(3)?,
                        version: r.get(4)?,
                        digest: r.get(5)?,
                        summary: r.get(6)?,
                        source_ref: r.get(7)?,
                        git_revision: r.get(8)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(q);
        for (id, c) in rows {
            let key = format!("checkpoint:{}:{}:{}", c.node_id, c.generation, c.version);
            let payload = serde_json::to_vec(&(&c, Vec::<crate::store::ArtifactInput>::new()))
                .map_err(|_| Error::Invalid("legacy checkpoint"))?;
            let digest = format!("{:x}", Sha256::digest(&payload));
            let event:i64=tx.query_row("SELECT COALESCE(MAX(id),0) FROM map_events WHERE node_id=? AND kind='checkpoint' AND detail=?",rusqlite::params![c.node_id,format!("{}:{}",c.generation,c.version)],|r|r.get(0))?;
            tx.execute(
                "INSERT INTO map_submissions VALUES(?,?,?,?,?)",
                rusqlite::params![key, c.node_id, digest, id, event],
            )?;
        }
    }
    tx.execute_batch(include_str!("takeover.sql"))?;
    install_legacy_worker_snapshot(&tx, version)?;
    navigation::rebuild(&tx)?;
    tx.execute(
        "INSERT INTO map_schema_migrations(version,applied_at) VALUES(?,unixepoch())",
        [SCHEMA_VERSION],
    )?;
    integrity(&tx)?;
    tx.commit()?;
    db.pragma_update(None, "foreign_keys", "ON")?;
    integrity(&db)?;
    Ok(())
}
impl MapStore {
    pub fn backup_to(&self, destination: &Path) -> Result<()> {
        backup(self, destination)
    }
    pub fn rebuild_projections(&self) -> Result<()> {
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut stmt =
            tx.prepare("SELECT name FROM sqlite_master WHERE type='index' AND name GLOB 'map_*'")?;
        let names = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        for name in names {
            tx.execute_batch(&format!("REINDEX \"{}\"", name.replace('"', "\"\"")))?;
        }
        navigation::rebuild(&tx)?;
        integrity(&tx)?;
        tx.commit()?;
        Ok(())
    }
}
