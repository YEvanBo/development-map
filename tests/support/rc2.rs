// Model a populated v10 fixture before running the real migration.
// Only call before intake: dropping an enrolled fixture would hide a test error.
pub fn migrate_fixture(store: &development_map::store::MapStore) {
    let db = store.connect(false).unwrap();
    let enrolled: i64 = db.query_row(
        "SELECT (SELECT count(*) FROM map_host_routes) + (SELECT count(*) FROM map_goal_checkpoints)
        + (SELECT count(*) FROM map_checkpoint_references) + (SELECT count(*) FROM map_rc2_nodes)",
        [], |r| r.get(0)).unwrap();
    assert_eq!(enrolled, 0);
    db.execute_batch(
        "DROP TABLE map_rc2_workers; DROP TABLE map_rc2_nodes;
        DROP TABLE map_checkpoint_references; DROP TABLE map_goal_checkpoints;
        DROP TABLE map_host_routes; DROP INDEX map_node_goal_events;
        UPDATE map_schema_migrations SET version=10;",
    )
    .unwrap();
    drop(db);
    store.migrate().unwrap();
}
