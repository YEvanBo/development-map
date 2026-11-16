// Copyright (c) 2026 EvanBo. Single-agent onboarding, no scheduler or bridge.
use development_map::{
    agent::Resume,
    identity::Binding,
    store::{CheckpointInput, MapStore, NodeInput},
};
use sha2::{Digest, Sha256};
use std::{error::Error, io::Write, path::PathBuf};
fn main() -> Result<(), Box<dyn Error>> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: cargo run --example solo -- <new-directory>")?,
    );
    std::fs::create_dir(&dir)?;
    let store = MapStore::new(dir.join("map.sqlite"));
    let db = |e| format!("{e:?}");
    store.migrate().map_err(db)?;
    std::fs::write(dir.join("architecture.md"),"Author: EvanBo\nGoal: demonstrate independent context recovery.\nConstraint: map records facts and never executes project tasks.\n")?;
    store
        .project("demo", "Solo project", "architecture.md")
        .map_err(db)?;
    store
        .adopt_architecture(
            "demo",
            "v1",
            "architecture.md",
            &format!(
                "{:x}",
                Sha256::digest(std::fs::read(dir.join("architecture.md"))?)
            ),
        )
        .map_err(db)?;
    store
        .node(&NodeInput {
            id: "demo/work".into(),
            project_id: "demo".into(),
            parent_id: None,
            kind: "work_package".into(),
            title: "First work package".into(),
            goal: "Recover this node and inspect checkpoint.md".into(),
            sort_key: "1".into(),
            plan_state: "active".into(),
            owner: Some("EvanBo".into()),
            generation: 1,
            architecture_ref: Some("architecture.md".into()),
        })
        .map_err(db)?;
    let token = uuid::Uuid::new_v4().to_string();
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(dir.join("agent-token.txt"))?
        .write_all(token.as_bytes())?;
    store
        .bind_identity(
            &token,
            &Binding {
                subject: "EvanBo".into(),
                project_id: "demo".into(),
                task_id: "solo-session".into(),
                owner: "EvanBo".into(),
                role: "worker".into(),
                node_id: Some("demo/work".into()),
                generation: Some(1),
                source_ref: "local-operator".into(),
                expires_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs() as i64
                    + 3600,
            },
        )
        .map_err(db)?;
    let checkpoint=b"Author: EvanBo\nDone: independent project initialized.\nRemaining: real project implementation and verification.\nNext: read architecture.md. No product acceptance.\n";
    std::fs::write(dir.join("checkpoint.md"), checkpoint)?;
    store.checkpoint(&CheckpointInput{node_id:"demo/work".into(),owner:"EvanBo".into(),generation:1,version:1,digest:format!("{:x}",Sha256::digest(checkpoint)),summary:"Initialized context. Remaining: implement real project. Next: read architecture.md.".into(),source_ref:"checkpoint.md".into(),git_revision:None}).map_err(db)?;
    let request: Resume = serde_json::from_value(serde_json::json!({"project_id":"demo"}))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&store.agent_resume(&token, &request).map_err(db)?)?
    );
    println!(
        "Token saved locally: {}",
        dir.join("agent-token.txt").display()
    );
    Ok(())
}
