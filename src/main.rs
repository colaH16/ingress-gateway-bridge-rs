// SPDX-License-Identifier: Apache-2.0
use ingress_gateway_bridge_rs::{
    Error, class_name, convert, model::Input, route::HttpRoute, validate_binding,
};
use serde_json::json;
use std::{
    io::{self, Read},
    process::ExitCode,
};
fn run() -> Result<u8, Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] || args == ["-h"] {
        println!(
            "Usage: ingress-gateway-bridge-rs [INPUT.json|-]\nOffline conversion of {{ingress, binding, services?, controllerName?}}.\nExit: 0 success, 1 input/config/I/O, 2 rejected, 3 missing dependency.\nThe binding is preselected; this command does not check IngressClass ownership."
        );
        return Ok(0);
    }
    if args.len() > 1 {
        return Err("expected at most one JSON input path".into());
    }
    let data = match args.first().map(String::as_str) {
        None | Some("-") => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s)?;
            s
        }
        Some(path) => std::fs::read_to_string(path)?,
    };
    let input: Input = serde_json::from_str(&data)?;
    validate_binding(&input.binding)?;
    match class_name(&input.ingress).and_then(|_| convert(&input)) {
        Ok(routes) => {
            // Typed fields serialize directly; no intermediate serde_json::Value.
            #[derive(serde::Serialize)]
            struct Output<'a> {
                routes: Vec<HttpRoute<'a>>,
            }
            println!("{}", serde_json::to_string_pretty(&Output { routes })?);
            Ok(0)
        }
        Err(e) => {
            println!("{}", serde_json::to_string(&json!({"error":e}))?);
            Ok(match e {
                Error::Rejected(_) => 2,
                Error::Dependency(_) => 3,
            })
        }
    }
}
fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
