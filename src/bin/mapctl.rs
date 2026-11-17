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
fn operator_request<T: serde::de::DeserializeOwned>() -> Result<T, String> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("operator request exceeds 64 KiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid operator request JSON".into())
}
fn run() -> Result<serde_json::Value, String> {
    let a = env::args().collect::<Vec<_>>();
    if a.get(1).map(String::as_str) == Some("status") {
        if a.len() == 2 {
            return Ok(development_map::diagnostics::program());
        }
        if !(a.len() == 4 || a.len() == 5) {
            return Err("usage: mapctl status [<db> <project_id> [node_id]]".into());
        }
        let token = env::var("MAP_AGENT_TOKEN").map_err(|_| "MAP_AGENT_TOKEN required")?;
        let request = development_map::agent::Resume {
            project_id: a[3].clone(),
            node_id: a.get(4).cloned(),
            ..Default::default()
        };
        return development_map::diagnostics::snapshot(&MapStore::new(&a[2]), &token, &request)
            .map_err(|e| development_map::agent::client_error(&e).to_string());
    }
    if a.len() < 3 {
        return Err("usage: mapctl <project|architecture-node|architecture-edge|dependency|trace|grant-reference|revoke-reference|repair-host|lookup-host|intake|assess-risk|record-risk|record-reminder|revoke-goal|confirm-goal|scope-check|scope-report|migrate|rebuild|backup|restore|import-speaker|import-harness|adopt|node|handoff|bind|revoke|register-repo|link-repo|submit-resources|resources|scan-git> <db> [arguments]".into());
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
            let token = env::var("MAP_AGENT_TOKEN").map_err(|_| "manager credential required")?;
            let bundle = operator_request()?;
            return s
                .submit_as_manager(&bundle, &token)
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
        "grant-reference" => {
            return s
                .grant_reference(&operator_request()?)
                .map(|id| json!({"reference_id":id}))
                .map_err(|e| format!("{e:?}"))
        }
        "revoke-reference" => s.revoke_reference(&operator_request()?),
        "assess-risk" => {
            return s
                .assess_risk(&operator_request()?)
                .map_err(|e| format!("{e:?}"))
        }
        "record-risk" => s.record_risk(&operator_request()?),
        "record-reminder" => s.record_reminder(&operator_request()?),
        "repair-host" => s.repair_host(&operator_request()?),
        "lookup-host" => {
            let key = operator_request()?;
            return s
                .host_target(&key)
                .map(|candidate| match candidate {
                    Some((project, node, revision)) => {
                        json!({"candidate":{"project_id":project,"node_id":node,"revision":revision}})
                    }
                    None => json!({"candidate":null}),
                })
                .map_err(|e| format!("{e:?}"));
        }
        "intake" => {
            let request = operator_request()?;
            return s
                .intake(&request)
                .map(|id| json!({"confirmation_event":id}))
                .map_err(|e| format!("{e:?}"));
        }
        "scope-check" => {
            return s
                .scope_check(
                    &env::var("MAP_AGENT_TOKEN").map_err(|_| "MAP_AGENT_TOKEN required")?,
                    &operator_request()?,
                )
                .map_err(|e| format!("{e:?}"))
        }
        "scope-report" => {
            return s
                .scope_report(
                    &env::var("MAP_AGENT_TOKEN").map_err(|_| "MAP_AGENT_TOKEN required")?,
                    &operator_request()?,
                )
                .map_err(|e| format!("{e:?}"))
        }
        "revoke-goal" => s.revoke_goal(&operator_request()?),
        "confirm-goal" => {
            let record = operator_request()?;
            return s
                .confirm_goal(&record)
                .map(|id| json!({"event_id":id}))
                .map_err(|e| format!("{e:?}"));
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
            let scope = input
                .get("host_scope")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|_| "invalid host scope")?;
            s.bind_scoped(
                input["token"].as_str().ok_or("token required")?,
                &b,
                scope.as_ref(),
            )
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
