// SPDX-License-Identifier: Apache-2.0
use anyhow::{Result, bail};
use ingress_gateway_bridge_rs::controller::{Config, runtime};
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut path = "bridge.yaml".to_string();
    let mut dry_run = true;
    let mut once = false;
    let mut single = false;
    let mut health = "0.0.0.0:8081".to_string();
    for arg in std::env::args().skip(1) {
        if let Some(v) = arg.strip_prefix("--config=") {
            path = v.into();
        } else if let Some(v) = arg.strip_prefix("--dry-run=") {
            dry_run = v.parse()?;
        } else if let Some(v) = arg.strip_prefix("--health-probe-bind-address=") {
            health = if v.starts_with(':') {
                format!("0.0.0.0{v}")
            } else {
                v.into()
            };
        } else if arg == "--once" {
            once = true;
        } else if arg == "--single-writer=true" {
            single = true;
        } else if arg == "--leader-elect=false" {
        } else if arg == "--help" {
            println!(
                "bridge-controller --config=bridge.yaml [--dry-run=true] [--once] [--single-writer=true] [--health-probe-bind-address=:8081]\nWriter requires one replica, Recreate strategy and no other controller writing the same classes. Leader election is not implemented."
            );
            return Ok(());
        } else {
            bail!("unsupported argument: {arg}");
        }
    }
    anyhow::ensure!(
        dry_run || single,
        "writer requires --single-writer=true and an exclusive single replica deployment"
    );
    anyhow::ensure!(!once || dry_run, "--once is read-only");
    let mut cfg: Config = serde_yaml::from_str(&std::fs::read_to_string(path)?)?;
    if cfg.controller_name.is_empty() {
        cfg.controller_name = ingress_gateway_bridge_rs::model::default_controller();
    }
    cfg.validate()?;
    runtime::run(cfg, dry_run, once, &health).await
}
