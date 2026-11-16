// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
//! Offline/local operator entry point. Does not stop or restart any service.
use development_map::{
    handoff::Handoff,
    identity::Binding,
    import, recovery,
    store::{MapStore, NodeInput},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    env,
    io::{self, Read},
    path::Path,
};
fn run() -> Result<serde_json::Value, String> {
    let a = env::args().collect::<Vec<_>>();
    if a.len() < 3 {
        return Err("usage: mapctl <project|architecture-node|architecture-edge|dependency|trace|migrate|rebuild|backup|restore|import-speaker|import-harness|adopt|node|handoff|bind|revoke|register-repo|link-repo|submit-resources|resources|scan-git> <db> [arguments]".into());
    }
    let s = MapStore::new(&a[2]);
    let arg = |i| {
        a.get(i)
            .map(String::as_str)
            .ok_or("missing argument".to_string())
    };
    let result = match a[1].as_str() {
        "migrate" => s.migrate(),
        "project" => s.project(arg(3)?, arg(4)?, arg(5)?),
        "architecture-node" => s.architecture_node(
            arg(3)?,
            arg(4)?,
            if arg(5)? == "-" { None } else { Some(arg(5)?) },
            arg(6)?,
            arg(7)?,
            arg(8)?,
            arg(9)?,
        ),
        "architecture-edge" => s.architecture_edge(arg(3)?, arg(4)?, arg(5)?, arg(6)?),
        "dependency" => s.dependency(arg(3)?, arg(4)?),
        "trace" => s.trace(arg(3)?, arg(4)?, arg(5)?, arg(6)?),
        "import-local" => {
            let mut input = String::new();
            io::stdin()
                .take(12 * 1024 * 1024 + 1)
                .read_to_string(&mut input)
                .map_err(|e| e.to_string())?;
            let snapshot: development_map::local_source::Snapshot =
                serde_json::from_str(&input).map_err(|e| e.to_string())?;
            return s
                .import_local_source(&snapshot)
                .map_err(|e| format!("{e:?}"));
        }
        "local-failure" => s.local_source_failure(arg(3)?, arg(4)?),
        "rebuild" => s.rebuild_projections(),
        "adopt" => s.adopt_architecture_full(arg(3)?, arg(4)?, arg(5)?, arg(6)?, arg(7)?, arg(8)?),
        "handoff" => {
            let token = env::var("MAP_AGENT_TOKEN")
                .map_err(|_| "MAP_AGENT_TOKEN required for handoff".to_string())?;
            let mut input = String::new();
            io::stdin()
                .take(65537)
                .read_to_string(&mut input)
                .map_err(|e| e.to_string())?;
            if input.len() > 65536 {
                return Err("handoff request exceeds 64 KiB".into());
            }
            let handoff: Handoff = serde_json::from_str(&input).map_err(|e| e.to_string())?;
            return s
                .handoff(&handoff, Some(&token))
                .map(|generation| json!({"generation":generation}))
                .map_err(|e| format!("{e:?}"));
        }
        "node" => {
            let mut input = String::new();
            io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| e.to_string())?;
            let node: NodeInput = serde_json::from_str(&input).map_err(|e| e.to_string())?;
            s.node(&node)
        }
        "backup" => s.backup_to(Path::new(arg(3)?)),
        "restore" => recovery::restore(Path::new(&a[2]), Path::new(arg(3)?)).map(|_| ()),
        "link-repo" => s.register_project_repository(arg(3)?, arg(4)?),
        "resources" => {
            return s
                .read(|tx| {
                    development_map::resources::manifest(
                        tx,
                        arg(3)
                            .map_err(|_| development_map::store::Error::Invalid("node required"))?,
                        None,
                    )
                })
                .map_err(|e| format!("{e:?}"));
        }
        "submit-resources" => {
            let mut input = String::new();
            io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| e.to_string())?;
            let bundle: development_map::submission::Submission =
                serde_json::from_str(&input).map_err(|e| e.to_string())?;
            return s
                .submit(&bundle)
                .and_then(|r| {
                    serde_json::to_value(r)
                        .map_err(|_| development_map::store::Error::Invalid("receipt"))
                })
                .map_err(|e| format!("{e:?}"));
        }
        "register-repo" => s.register_repository(arg(3)?, Path::new(arg(4)?), arg(5)?),
        "scan-git" => {
            return s
                .scan_git(arg(3)?, arg(4)?, arg(5)?)
                .map(|count| json!({"observations":count}))
                .map_err(|e| format!("{e:?}"))
        }
        "bind" => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| e.to_string())?;
            let input: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let b: Binding =
                serde_json::from_value(input["binding"].clone()).map_err(|e| e.to_string())?;
            s.bind_identity(input["token"].as_str().ok_or("token required")?, &b)
        }
        "revoke" => {
            let mut token = String::new();
            io::stdin()
                .read_to_string(&mut token)
                .map_err(|e| e.to_string())?;
            let token = token.trim_end_matches(['\r', '\n']);
            if token.is_empty() {
                return Err("token required".into());
            }
            s.revoke_identity(token)
        }
        "import-speaker" | "import-harness" => {
            let path = arg(3)?;
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            let digest = format!("{:x}", Sha256::digest(&bytes));
            let source = format!("{path}#sha256={digest}");
            let manifest = if a[1] == "import-speaker" {
                import::speaker_ledger(&bytes, &source)
            } else {
                import::harness_tree(&bytes, &source)
            }
            .map_err(|e| format!("{e:?}"))?;
            let created = s.import_manifest(&manifest).map_err(|e| format!("{e:?}"))?;
            return Ok(
                json!({"created":created,"project_id":manifest.project_id,"nodes":manifest.nodes.len(),"source_sha256":digest,"adoption":"unadopted","acceptance":"not_inferred"}),
            );
        }
        _ => return Err("unknown command".into()),
    };
    result.map_err(|e| format!("{e:?}"))?;
    Ok(json!({"ok":true}))
}
fn main() {
    match run() {
        Ok(v) => println!("{v}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1)
        }
    }
}
