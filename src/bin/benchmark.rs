// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
use development_map::store::{self, MapStore};
use rusqlite::params;
use std::{env, time::Instant};
fn percentile(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(|a, b| a.total_cmp(b));
    xs[(xs.len() * 95 / 100).min(xs.len() - 1)]
}
type Query<'a> = (
    &'static str,
    Box<dyn Fn() -> store::Result<serde_json::Value> + 'a>,
);
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: benchmark <new-db-path>");
        std::process::exit(2)
    }
    let s = MapStore::new(&args[1]);
    s.migrate().unwrap();
    let mut db = s.connect(false).unwrap();
    let start = Instant::now();
    let tx = db.transaction().unwrap();
    tx.execute(
        "INSERT OR IGNORE INTO map_repositories VALUES('repo','/benchmark','fixture')",
        [],
    )?;
    for p in 0..100 {
        tx.execute(
            "INSERT INTO map_projects(id,name,source_ref,updated_at) VALUES(?,?,?,0)",
            params![format!("p{p:03}"), format!("Project {p}"), "fixture"],
        )?;
    }
    for p in 0..100 {
        let project = format!("p{p:03}");
        tx.execute(
            "INSERT INTO map_project_repositories VALUES(?,'repo')",
            [&project],
        )?;
        tx.execute("INSERT INTO map_architectures(project_id,version,source_ref,digest,adopted_at) VALUES(?,'v1','design','1234567890123456',0)",[&project])?;
        tx.execute("INSERT INTO map_architecture_revisions(project_id,version,source_ref,digest,variant,adoption_ref,adopted_at) VALUES(?,'v1','design','1234567890123456','default','fixture',0)",[&project])?;
        for a in 0..10 {
            tx.execute("INSERT INTO map_arch_nodes(id,project_id,arch_version,parent_id,title,kind,source_ref,sort_key) VALUES(?,?,'v1',NULL,?,'component','design',?)",params![format!("a{p:03}-{a:02}"),project,format!("Component {a}"),format!("{a:02}")])?;
        }
    }
    for i in 0..100_000 {
        let p = i / 1000;
        let id = format!("n{i:06}");
        let parent = if i % 1000 < 50 {
            None
        } else {
            Some(format!("n{:06}", i - (i % 1000)))
        };
        tx.execute("INSERT INTO map_dev_nodes(id,project_id,parent_id,kind,title,goal,sort_key,plan_state,owner,generation,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,0)",params![id,format!("p{p:03}"),parent,"module",format!("Node {i}"),"goal",format!("{i:06}"),"active","worker",1])?;
    }
    for i in 0..100_000 {
        let p = i / 1000;
        let a = (i % 1000) / 100;
        tx.execute("INSERT INTO map_arch_traces(arch_node_id,arch_version,dev_node_id,relation,source_ref) VALUES(? ,'v1',?,'implements','fixture')",params![format!("a{p:03}-{a:02}"),format!("n{i:06}")])?;
    }
    for i in 0..1_000_000 {
        let node = format!("n{:06}", i % 100_000);
        let path = format!("src/f{i:07}.rs");
        tx.execute("INSERT INTO map_artifacts(project_id,repo_key,revision,path,kind,verification) VALUES(?,'repo','abc',?,'file','verified')",params![format!("p{:03}",(i%100000)/1000),path])?;
        tx.execute(
            "INSERT INTO map_node_artifacts(node_id,artifact_id,provenance) VALUES(?,?,'declared')",
            params![node, i + 1],
        )?;
    }
    for i in 0..1000 {
        tx.execute("INSERT INTO map_events(project_id,node_id,kind,detail,created_at) VALUES('p000',?,'checkpoint','',0)",[format!("n{i:06}")])?;
    }
    tx.commit()?;
    s.rebuild_projections().unwrap();
    println!("seed_seconds={:.2}", start.elapsed().as_secs_f64());
    let queries: [Query<'_>; 10] = [
        (
            "projects",
            Box::new(|| s.read(|tx| store::list_projects(tx, "", 50))),
        ),
        (
            "children",
            Box::new(|| s.read(|tx| store::children(tx, "project:p000", "", 50, None))),
        ),
        (
            "navigation",
            Box::new(|| {
                s.read(|tx| {
                    development_map::navigation::children(
                        tx,
                        "p000",
                        None,
                        "architecture:p000:v1",
                        "",
                        50,
                        None,
                    )
                })
            }),
        ),
        (
            "architecture",
            Box::new(|| s.read(|tx| store::architecture(tx, "p000"))),
        ),
        (
            "architecture_children",
            Box::new(|| s.read(|tx| store::architecture_children(tx, "p000", None, "", 50))),
        ),
        (
            "architecture_neighborhood",
            Box::new(|| s.read(|tx| store::architecture_neighborhood(tx, "a000-00", 50))),
        ),
        (
            "node",
            Box::new(|| s.read(|tx| store::node_detail(tx, "n000500"))),
        ),
        (
            "changes",
            Box::new(|| s.read(|tx| store::changes(tx, "p000", 0, 100))),
        ),
        (
            "summary",
            Box::new(|| {
                s.read(|tx| development_map::navigation::summary(tx, "p000", None, "project:p000"))
            }),
        ),
        (
            "focus",
            Box::new(|| {
                s.read(|tx| development_map::navigation::focus(tx, "p000", None, "dev:n000500"))
            }),
        ),
    ];
    for (name, f) in queries {
        let mut timings = Vec::new();
        let mut bytes = 0;
        for _ in 0..100 {
            let t = Instant::now();
            let v = f().unwrap();
            timings.push(t.elapsed().as_secs_f64() * 1000.);
            bytes = serde_json::to_vec(&v).unwrap().len();
        }
        println!("{name}: p95_ms={:.2} bytes={bytes}", percentile(timings));
    }
    let size = std::fs::metadata(&args[1])?.len();
    println!("database_bytes={size}");
    Ok(())
}
