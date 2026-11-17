-- VNext schema 13. Operator-attested workflow; no native host identity claim.
CREATE TABLE IF NOT EXISTS map_approval_requests (
 id TEXT PRIMARY KEY,
 nonce TEXT NOT NULL UNIQUE,
 project_id TEXT NOT NULL REFERENCES map_projects(id),
 node_id TEXT NOT NULL REFERENCES map_dev_nodes(id),
 challenge TEXT NOT NULL,
 expires_at INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('pending','consumed','denied','cancelled','modified','explanation_required','revoked')),
 decision_digest TEXT,
 result TEXT
);
CREATE TABLE IF NOT EXISTS map_approval_evidence (
 provider TEXT NOT NULL,
 host_instance TEXT NOT NULL,
 conversation TEXT NOT NULL,
 message_id TEXT NOT NULL,
 source_reference TEXT NOT NULL UNIQUE,
 challenge_id TEXT NOT NULL UNIQUE REFERENCES map_approval_requests(id),
 PRIMARY KEY(provider,host_instance,conversation,message_id)
);
