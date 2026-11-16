CREATE TABLE IF NOT EXISTS map_schema_migrations(version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS map_navigation(
 project_id TEXT NOT NULL, revision TEXT NOT NULL, node_ref TEXT NOT NULL, parent_ref TEXT,
 kind TEXT NOT NULL, title TEXT NOT NULL, sort_key TEXT NOT NULL, source_ref TEXT,
 plan_state TEXT, development_state TEXT, acceptance_state TEXT, checkpoint_rowid INTEGER,
 node_version INTEGER NOT NULL DEFAULT 0, direct_children INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(project_id,revision,node_ref));
CREATE INDEX IF NOT EXISTS map_navigation_parent ON map_navigation(project_id,revision,parent_ref,sort_key,node_ref);
CREATE TABLE IF NOT EXISTS map_rollups(project_id TEXT NOT NULL,revision TEXT NOT NULL,node_ref TEXT NOT NULL,node_kind TEXT NOT NULL,total INTEGER NOT NULL DEFAULT 0,planned INTEGER NOT NULL DEFAULT 0,active INTEGER NOT NULL DEFAULT 0,blocked INTEGER NOT NULL DEFAULT 0,paused INTEGER NOT NULL DEFAULT 0,submitted INTEGER NOT NULL DEFAULT 0,pending INTEGER NOT NULL DEFAULT 0,accepted INTEGER NOT NULL DEFAULT 0,stale INTEGER NOT NULL DEFAULT 0,rejected INTEGER NOT NULL DEFAULT 0,event_id INTEGER NOT NULL,PRIMARY KEY(project_id,revision,node_ref,node_kind));
CREATE TABLE IF NOT EXISTS map_submissions(submission_key TEXT NOT NULL,node_id TEXT NOT NULL,payload_digest TEXT NOT NULL,checkpoint_rowid INTEGER NOT NULL UNIQUE,event_id INTEGER NOT NULL,PRIMARY KEY(node_id,submission_key));
CREATE TABLE IF NOT EXISTS map_checkpoint_artifacts(checkpoint_rowid INTEGER NOT NULL,artifact_id INTEGER NOT NULL REFERENCES map_artifacts(id),provenance TEXT NOT NULL,PRIMARY KEY(checkpoint_rowid,artifact_id));
CREATE TABLE IF NOT EXISTS map_plan_revisions(project_id TEXT NOT NULL,revision TEXT NOT NULL,architecture_revision TEXT NOT NULL,source_ref TEXT NOT NULL,digest TEXT NOT NULL,manifest TEXT NOT NULL,created_at INTEGER NOT NULL,PRIMARY KEY(project_id,revision));
CREATE TABLE IF NOT EXISTS map_repositories(repo_key TEXT PRIMARY KEY,path TEXT NOT NULL,source_ref TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS map_git_observations(repo_key TEXT NOT NULL,from_revision TEXT NOT NULL,to_revision TEXT NOT NULL,ordinal INTEGER NOT NULL,status TEXT NOT NULL,old_path TEXT,new_path TEXT NOT NULL,PRIMARY KEY(repo_key,from_revision,to_revision,ordinal));
CREATE TABLE IF NOT EXISTS map_git_scans(repo_key TEXT NOT NULL,from_revision TEXT NOT NULL,to_revision TEXT NOT NULL,count INTEGER NOT NULL,PRIMARY KEY(repo_key,from_revision,to_revision));
CREATE INDEX IF NOT EXISTS map_checkpoint_page ON map_checkpoint_index(node_id);
CREATE TABLE IF NOT EXISTS map_identities(token_hash TEXT PRIMARY KEY,binding TEXT NOT NULL,revoked INTEGER NOT NULL DEFAULT 0);


CREATE TABLE IF NOT EXISTS map_submission_identity(checkpoint_id INTEGER PRIMARY KEY REFERENCES map_checkpoint_index(id),subject TEXT NOT NULL,task_id TEXT NOT NULL,owner TEXT NOT NULL,source_ref TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS map_navigation_reference ON map_navigation(node_ref,project_id,revision);

CREATE TABLE IF NOT EXISTS map_project_repositories(project_id TEXT NOT NULL REFERENCES map_projects(id),repo_key TEXT NOT NULL REFERENCES map_repositories(repo_key),PRIMARY KEY(project_id,repo_key));
CREATE TABLE IF NOT EXISTS map_resource_sets(checkpoint_id INTEGER PRIMARY KEY REFERENCES map_checkpoint_index(id),manifest TEXT NOT NULL,hierarchy_state TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS map_resource_entries(checkpoint_id INTEGER NOT NULL REFERENCES map_resource_sets(checkpoint_id),resource_key TEXT NOT NULL,category TEXT NOT NULL,repo_key TEXT,revision TEXT,path TEXT,ownership TEXT NOT NULL,resource TEXT NOT NULL,PRIMARY KEY(checkpoint_id,resource_key));
CREATE INDEX IF NOT EXISTS map_resource_reverse ON map_resource_entries(repo_key,revision,path,checkpoint_id);

CREATE TABLE IF NOT EXISTS map_local_sources(project_id TEXT PRIMARY KEY REFERENCES map_projects(id),digest TEXT NOT NULL,version INTEGER NOT NULL,core_node TEXT NOT NULL,metadata TEXT NOT NULL,checked_at INTEGER NOT NULL,changed_at INTEGER NOT NULL,last_error TEXT,stale_after INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS map_local_nodes(node_id TEXT PRIMARY KEY REFERENCES map_dev_nodes(id),project_id TEXT NOT NULL,details TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS map_local_files(project_id TEXT NOT NULL,path TEXT NOT NULL,metadata TEXT NOT NULL,PRIMARY KEY(project_id,path));
CREATE TABLE IF NOT EXISTS map_local_links(project_id TEXT NOT NULL,node_id TEXT NOT NULL,path TEXT NOT NULL,category TEXT NOT NULL,PRIMARY KEY(project_id,node_id,path,category));
CREATE INDEX IF NOT EXISTS map_local_reverse ON map_local_links(project_id,path,node_id);
CREATE TABLE IF NOT EXISTS map_local_history(project_id TEXT NOT NULL,version INTEGER NOT NULL,digest TEXT NOT NULL,observed_at INTEGER NOT NULL,snapshot TEXT NOT NULL,PRIMARY KEY(project_id,version));


CREATE VIEW IF NOT EXISTS map_local_node_artifacts AS SELECT l.* FROM map_node_artifacts l JOIN map_dev_nodes n ON n.id=l.node_id JOIN map_artifacts a ON a.id=l.artifact_id AND a.project_id=n.project_id JOIN map_project_repositories r ON (r.project_id,r.repo_key)=(n.project_id,a.repo_key);
CREATE VIEW IF NOT EXISTS map_local_checkpoint_artifacts AS SELECT l.* FROM map_checkpoint_artifacts l JOIN map_checkpoint_index c ON c.id=l.checkpoint_rowid JOIN map_dev_nodes n ON n.id=c.node_id JOIN map_artifacts a ON a.id=l.artifact_id AND a.project_id=n.project_id JOIN map_project_repositories r ON (r.project_id,r.repo_key)=(n.project_id,a.repo_key);

CREATE TABLE IF NOT EXISTS map_legacy_artifact_verifications(
    artifact_id INTEGER PRIMARY KEY REFERENCES map_artifacts(id),
    verification TEXT NOT NULL,
    content_digest TEXT
);
