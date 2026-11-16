// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
//! Local authenticated client. No provisioning, migrations, shell execution or bridge calls.
use development_map::{agent::Resume, store::MapStore, submission::Submission};
use std::{
    env,
    io::{self, Read},
};
fn run() -> Result<serde_json::Value, String> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 3 || !matches!(args[1].as_str(), "resume" | "submit") {
        return Err("usage: MAP_AGENT_TOKEN=... map-agent <resume|submit> <existing-map.sqlite> < request.json".into());
    }
    if !std::path::Path::new(&args[2]).is_file() {
        return Err("existing map database required".into());
    }
    let token = env::var("MAP_AGENT_TOKEN")
        .map_err(|_| "MAP_AGENT_TOKEN required; provision through trusted host operator")?;
    let mut data = String::new();
    io::stdin()
        .take(65537)
        .read_to_string(&mut data)
        .map_err(|_| "cannot read request")?;
    if data.len() > 65536 {
        return Err("request exceeds 64 KiB".into());
    }
    let store = MapStore::new(&args[2]);
    let value = if args[1] == "resume" {
        let request: Resume = serde_json::from_str(&data).map_err(|_| "invalid resume JSON")?;
        store
            .agent_resume(&token, &request)
            .map_err(|e| format!("{e:?}"))?
    } else {
        let input: Submission =
            serde_json::from_str(&data).map_err(|_| "invalid submission JSON")?;
        serde_json::to_value(
            store
                .submit_bound(&input, Some(&token))
                .map_err(|e| format!("{e:?}"))?,
        )
        .map_err(|_| "receipt encoding")?
    };
    Ok(value)
}
fn main() {
    match run() {
        Ok(v) => println!("{v}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
