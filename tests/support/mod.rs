// Explicit resource declarations for logical state-machine test nodes; not source verification fixtures.
pub fn logical_manifest() -> development_map::resources::Manifest {
    development_map::resources::Manifest {
        architecture_revision: "v1".into(),
        hierarchy_exception: "".into(),
        entries: ["directory", "code", "document", "database", "checkpoint"]
            .iter()
            .map(|category| development_map::resources::Resource {
                key: (*category).into(),
                category: (*category).into(),
                repo_key: None,
                revision: None,
                path: None,
                ownership: "primary".into(),
                required: true,
                not_applicable: true,
                reason: "Logical test node; authoritative recovery is its checkpoint record".into(),
                database: None,
            })
            .collect(),
    }
}
