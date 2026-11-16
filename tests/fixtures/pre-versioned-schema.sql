CREATE TABLE IF NOT EXISTS map_projects(id TEXT PRIMARY KEY,name TEXT NOT NULL,source_ref TEXT NOT NULL DEFAULT '',updated_at REAL NOT NULL);
CREATE TABLE IF NOT EXISTS map_dev_nodes(id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES map_projects(id),parent_id TEXT REFERENCES map_dev_nodes(id),kind TEXT NOT NULL CHECK(kind IN ('domain','module','submodule','function','work_package')),title TEXT NOT NULL,goal TEXT NOT NULL,sort_key TEXT NOT NULL,plan_state TEXT NOT NULL,architecture_ref TEXT,owner TEXT,generation INTEGER NOT NULL DEFAULT 0,updated_at REAL NOT NULL);
CREATE INDEX IF NOT EXISTS map_dev_nodes_parent ON map_dev_nodes(project_id,parent_id,sort_key,id);
CREATE INDEX IF NOT EXISTS map_dev_nodes_plan ON map_dev_nodes(project_id,plan_state,id);
CREATE TABLE IF NOT EXISTS map_edges(from_id TEXT NOT NULL REFERENCES map_dev_nodes(id),to_id TEXT NOT NULL REFERENCES map_dev_nodes(id),kind TEXT NOT NULL,PRIMARY KEY(from_id,to_id,kind));
CREATE INDEX IF NOT EXISTS map_edges_reverse ON map_edges(to_id,kind);
CREATE TABLE IF NOT EXISTS map_checkpoint_index(node_id TEXT NOT NULL REFERENCES map_dev_nodes(id),owner TEXT NOT NULL,generation INTEGER NOT NULL,version INTEGER NOT NULL,digest TEXT NOT NULL,summary TEXT NOT NULL,source_ref TEXT NOT NULL,git_revision TEXT,arch_version TEXT NOT NULL,created_at REAL NOT NULL,UNIQUE(node_id,owner,generation,version));
CREATE INDEX IF NOT EXISTS map_checkpoint_node ON map_checkpoint_index(node_id,generation DESC,version DESC);
CREATE TABLE IF NOT EXISTS map_acceptances(node_id TEXT NOT NULL REFERENCES map_dev_nodes(id),generation INTEGER NOT NULL,version INTEGER NOT NULL,digest TEXT NOT NULL,decision TEXT NOT NULL,evidence_level TEXT NOT NULL,source_ref TEXT NOT NULL,created_at REAL NOT NULL,PRIMARY KEY(node_id,generation,version));
CREATE TABLE IF NOT EXISTS map_artifacts(id INTEGER PRIMARY KEY,repo_key TEXT NOT NULL,revision TEXT NOT NULL,path TEXT NOT NULL,kind TEXT NOT NULL,verification TEXT NOT NULL,content_digest TEXT,UNIQUE(repo_key,revision,path,kind));
CREATE INDEX IF NOT EXISTS map_artifacts_lookup ON map_artifacts(repo_key,revision,path);
CREATE TABLE IF NOT EXISTS map_node_artifacts(node_id TEXT NOT NULL REFERENCES map_dev_nodes(id),artifact_id INTEGER NOT NULL REFERENCES map_artifacts(id),provenance TEXT NOT NULL,PRIMARY KEY(node_id,artifact_id));
CREATE INDEX IF NOT EXISTS map_node_artifacts_reverse ON map_node_artifacts(artifact_id,node_id);
CREATE TABLE IF NOT EXISTS map_events(id INTEGER PRIMARY KEY AUTOINCREMENT,project_id TEXT NOT NULL,node_id TEXT,kind TEXT NOT NULL,detail TEXT NOT NULL,created_at REAL NOT NULL);
CREATE INDEX IF NOT EXISTS map_events_project ON map_events(project_id,id);
CREATE TABLE IF NOT EXISTS map_architectures(project_id TEXT PRIMARY KEY REFERENCES map_projects(id),version TEXT NOT NULL,source_ref TEXT NOT NULL,digest TEXT NOT NULL,adopted_at REAL NOT NULL);
CREATE TABLE IF NOT EXISTS map_arch_nodes(id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES map_projects(id),arch_version TEXT NOT NULL,parent_id TEXT REFERENCES map_arch_nodes(id),title TEXT NOT NULL,kind TEXT NOT NULL,source_ref TEXT NOT NULL,sort_key TEXT NOT NULL,requirement_id TEXT,required INTEGER NOT NULL DEFAULT 1);
CREATE INDEX IF NOT EXISTS map_arch_nodes_parent ON map_arch_nodes(project_id,parent_id,sort_key,id);
CREATE TABLE IF NOT EXISTS map_arch_traces(arch_node_id TEXT NOT NULL REFERENCES map_arch_nodes(id),dev_node_id TEXT NOT NULL REFERENCES map_dev_nodes(id),relation TEXT NOT NULL,source_ref TEXT NOT NULL,PRIMARY KEY(arch_node_id,dev_node_id,relation));
CREATE INDEX IF NOT EXISTS map_arch_traces_dev ON map_arch_traces(dev_node_id,arch_node_id);
CREATE TABLE IF NOT EXISTS map_architecture_revisions(project_id TEXT NOT NULL REFERENCES map_projects(id),version TEXT NOT NULL,source_ref TEXT NOT NULL,digest TEXT NOT NULL,variant TEXT NOT NULL,adoption_ref TEXT NOT NULL,adopted_at REAL NOT NULL,PRIMARY KEY(project_id,version));
CREATE INDEX IF NOT EXISTS map_arch_traces_arch ON map_arch_traces(arch_node_id,dev_node_id);
CREATE VIEW IF NOT EXISTS map_nodes AS
 SELECT 'project:'||p.id AS node_ref,p.id AS project_id,NULL AS parent_ref,'project' AS kind,p.name AS title,p.id AS sort_key FROM map_projects p
 UNION ALL SELECT 'architecture:'||a.project_id||':'||a.version,a.project_id,'project:'||a.project_id,'architecture',a.version,a.version FROM map_architectures a
 UNION ALL SELECT 'arch:'||a.id,a.project_id,CASE WHEN a.parent_id IS NULL THEN 'architecture:'||a.project_id||':'||a.arch_version ELSE 'arch:'||a.parent_id END,a.kind,a.title,a.sort_key FROM map_arch_nodes a JOIN map_architectures b ON b.project_id=a.project_id AND b.version=a.arch_version
 UNION ALL SELECT 'dev:'||n.id,n.project_id,CASE WHEN n.parent_id IS NOT NULL THEN 'dev:'||n.parent_id ELSE COALESCE((SELECT 'arch:'||t.arch_node_id FROM map_arch_traces t JOIN map_arch_nodes a ON a.id=t.arch_node_id JOIN map_architectures b ON b.project_id=a.project_id AND b.version=a.arch_version WHERE t.dev_node_id=n.id AND t.relation='primary' LIMIT 1),'architecture:'||n.project_id||':'||(SELECT version FROM map_architectures WHERE project_id=n.project_id)) END,n.kind,n.title,n.sort_key FROM map_dev_nodes n;
CREATE INDEX IF NOT EXISTS map_dev_nodes_title ON map_dev_nodes(project_id,title,id);

CREATE TABLE IF NOT EXISTS map_architecture_edges(from_id TEXT NOT NULL REFERENCES map_arch_nodes(id),to_id TEXT NOT NULL REFERENCES map_arch_nodes(id),kind TEXT NOT NULL,source_ref TEXT NOT NULL,PRIMARY KEY(from_id,to_id,kind));
CREATE INDEX IF NOT EXISTS map_architecture_edges_reverse ON map_architecture_edges(to_id,kind);
