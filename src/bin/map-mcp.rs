// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
//! Newline JSON-RPC on stdin/stdout; credentials never appear on stdout.
use development_map::{
    mcp::{self, Session},
    store::MapStore,
};
use std::{
    env, fs,
    io::{self, BufRead, Read, Write},
    path::Path,
};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() == 2 && args[1] == "--version" {
        eprintln!("map-mcp {} by EvanBo", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if !(args.len() == 2 || (args.len() == 4 && args[2] == "--token-file")) {
        return Err(
            "usage: map-mcp <existing-map.sqlite> [--token-file <private-token-file>]".into(),
        );
    }
    if !Path::new(&args[1]).is_file() {
        return Err("existing map database required".into());
    }
    let token = if args.len() == 4 {
        let mut f = fs::File::open(&args[3])?;
        if !f.metadata()?.is_file() {
            return Err("token file must be regular".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if f.metadata()?.permissions().mode() & 0o077 != 0 {
                return Err("token file must have private permissions (0600)".into());
            }
        }
        let mut s = String::new();
        Read::by_ref(&mut f).take(4097).read_to_string(&mut s)?;
        if s.len() > 4096 {
            return Err("token file exceeds 4 KiB".into());
        }
        s.trim().to_owned()
    } else {
        env::var("MAP_AGENT_TOKEN").map_err(|_| "MAP_AGENT_TOKEN or --token-file required")?
    };
    if token.len() < 24 {
        return Err("invalid credential length".into());
    }
    let store = MapStore::new(&args[1]);
    let mut state = Session::default();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let mut line = Vec::new();
        let count = Read::by_ref(&mut input)
            .take(131073)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        if line.len() > 131072 {
            return Err("MCP message exceeds 128 KiB".into());
        }
        let response = match serde_json::from_slice(&line) {
            Ok(v) => state.handle(&store, &token, v),
            Err(_) => Some(mcp::parse_error()),
        };
        if let Some(v) = response {
            serde_json::to_writer(&mut output, &v)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}
fn main() {
    if run().is_err() {
        eprintln!("map-mcp failed; check arguments, private credential file, database, input bounds and pipe state");
        std::process::exit(1)
    }
}
