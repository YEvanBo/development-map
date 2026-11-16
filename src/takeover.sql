-- v11: legacy credentials remain valid for their assigned node. No routes are inferred.
CREATE TABLE IF NOT EXISTS map_host_routes (
    provider TEXT NOT NULL,
    workspace TEXT NOT NULL,
    task_id TEXT NOT NULL,
    project_id TEXT NOT NULL REFERENCES map_projects(id),
    node_id TEXT NOT NULL REFERENCES map_dev_nodes(id),
    revision INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY(provider, workspace, task_id)
);
CREATE INDEX IF NOT EXISTS map_node_goal_events
    ON map_events(project_id,node_id,id) WHERE kind='user_goal';
CREATE TABLE IF NOT EXISTS map_goal_checkpoints (
    event_id INTEGER PRIMARY KEY REFERENCES map_events(id),
    checkpoint_id INTEGER NOT NULL UNIQUE REFERENCES map_checkpoint_index(id)
);

CREATE TABLE IF NOT EXISTS map_checkpoint_references (
    id TEXT PRIMARY KEY,
    recipient TEXT NOT NULL REFERENCES map_dev_nodes(id),
    checkpoint_id INTEGER NOT NULL REFERENCES map_checkpoint_index(id),
    decision_ref TEXT NOT NULL,
    revoked_by TEXT,
    UNIQUE(recipient, checkpoint_id, decision_ref)
);
