-- Only the pre-intake migration populates these snapshots.
CREATE TABLE IF NOT EXISTS map_rc2_nodes (
    node_id TEXT PRIMARY KEY REFERENCES map_dev_nodes(id),
    owner TEXT NOT NULL,
    generation INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS map_rc2_workers (
    token_hash TEXT PRIMARY KEY REFERENCES map_identities(token_hash)
);
