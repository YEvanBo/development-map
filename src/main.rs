// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
use development_map::{store::MapStore, web::MapWeb};
use std::{env, path::Path};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() == 2 && matches!(args[1].as_str(), "--version" | "-V") {
        println!(
            "development-map {} by {} [{}]",
            env!("CARGO_PKG_VERSION"),
            development_map::AUTHOR,
            development_map::PROVENANCE_MARK
        );
        return Ok(());
    }
    if args.len() != 3 || !matches!(args[1].as_str(), "serve" | "serve-scoped") {
        eprintln!("usage: development-map serve <existing-sqlite-path>");
        std::process::exit(2)
    }
    let path = Path::new(&args[2]);
    if !path.is_file() {
        return Err("database must already exist".into());
    }
    let store = MapStore::new(path);
    let bearer = uuid::Uuid::new_v4().to_string();
    let mut web = MapWeb::new(store, bearer);
    if args[1] == "serve-scoped" {
        web = web.with_scoped_identities();
    }
    let code = web.issue_launch_code();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    if args[1] == "serve-scoped" {
        println!("{{\"url\":\"http://127.0.0.1:{}\"}}", addr.port());
    } else {
        println!("http://127.0.0.1:{}/map?code={}", addr.port(), code);
    }
    axum::serve(listener, web.router()).await?;
    Ok(())
}
