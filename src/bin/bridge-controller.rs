// SPDX-License-Identifier: Apache-2.0
use anyhow::{Context, Result};
use ingress_gateway_bridge_rs::{
    controller::{Config, runtime},
    settings::{ENV_KEYS, Settings},
};
use std::collections::BTreeMap;
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "bridge-controller [optional CLI overrides]\nConfiguration precedence: CLI > environment > defaults.\nBRIDGE_CONFIG                config path (binary: bridge.yaml; image: /etc/bridge/bridge.yaml)\nBRIDGE_DRY_RUN               true (default) or false\nBRIDGE_SINGLE_WRITER         false (default); must be true for writes\nBRIDGE_HEALTH_PROBE_BIND_ADDRESS  0.0.0.0:8081 (default)\nBRIDGE_ONCE                  false (default); true runs one read-only resync\nRUST_LOG                     info (default)\nCLI: --config=PATH --dry-run=BOOL --single-writer=BOOL --health-probe-bind-address=ADDR --once[=BOOL]\nUse one writer replica and Recreate; leader election is not implemented."
        );
        return Ok(());
    }
    let mut env = BTreeMap::new();
    for key in ENV_KEYS {
        match std::env::var(key) {
            Ok(v) => {
                env.insert(key.into(), v);
            }
            Err(std::env::VarError::NotPresent) => (),
            Err(e) => return Err(e).with_context(|| format!("invalid environment variable {key}")),
        }
    }
    let settings = Settings::parse(args, &env)?;
    let text = std::fs::read_to_string(&settings.config)
        .with_context(|| format!("read config {}", settings.config))?;
    let mut cfg: Config = serde_yaml::from_str(&text)?;
    if cfg.controller_name.is_empty() {
        cfg.controller_name = ingress_gateway_bridge_rs::model::default_controller();
    }
    cfg.validate()?;
    runtime::run(
        cfg,
        settings.dry_run,
        settings.once,
        &settings.health_address,
    )
    .await
}
