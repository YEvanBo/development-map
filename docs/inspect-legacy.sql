-- Run locally: sqlite3 -readonly map.sqlite < docs/inspect-legacy.sql
-- After schema v9 migration. No statements here change the database.
SELECT 'quarantined artifact',id,repo_key,revision,path FROM map_artifacts WHERE project_id IS NULL;
SELECT 'hidden node link',* FROM (SELECT * FROM map_node_artifacts EXCEPT SELECT * FROM map_local_node_artifacts);
SELECT 'hidden checkpoint link',* FROM (SELECT * FROM map_checkpoint_artifacts EXCEPT SELECT * FROM map_local_checkpoint_artifacts);
SELECT 'hidden architecture trace',* FROM (SELECT * FROM map_arch_traces EXCEPT SELECT * FROM map_local_traces);
SELECT 'hidden interface',* FROM (SELECT * FROM map_architecture_edges EXCEPT SELECT * FROM map_local_interfaces);

SELECT 'legacy verified claim',artifact_id,verification,content_digest FROM map_legacy_artifact_verifications;
