// SPDX-License-Identifier: Apache-2.0
//! Resolve CLI > environment > defaults without changing the process environment.
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;

pub const ENV_KEYS: [&str; 5] = [
    "BRIDGE_CONFIG",
    "BRIDGE_DRY_RUN",
    "BRIDGE_SINGLE_WRITER",
    "BRIDGE_HEALTH_PROBE_BIND_ADDRESS",
    "BRIDGE_ONCE",
];
#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    pub config: String,
    pub dry_run: bool,
    pub single_writer: bool,
    pub health_address: String,
    pub once: bool,
}
impl Settings {
    pub fn parse(
        args: impl IntoIterator<Item = String>,
        env: &BTreeMap<String, String>,
    ) -> Result<Self> {
        let mut values: BTreeMap<String, String> = [
            ("BRIDGE_CONFIG", "bridge.yaml"),
            ("BRIDGE_DRY_RUN", "true"),
            ("BRIDGE_SINGLE_WRITER", "false"),
            ("BRIDGE_HEALTH_PROBE_BIND_ADDRESS", "0.0.0.0:8081"),
            ("BRIDGE_ONCE", "false"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        for key in ENV_KEYS {
            if let Some(value) = env.get(key) {
                values.insert(key.into(), value.clone());
            }
        }
        for arg in args {
            if arg == "--once" {
                values.insert("BRIDGE_ONCE".into(), "true".into());
                continue;
            }
            // Compatibility with old Go manifests, but this implementation has no election.
            if arg == "--leader-elect=false" {
                continue;
            }
            let Some((flag, value)) = arg.split_once('=') else {
                bail!("unsupported argument: {arg}");
            };
            let key = match flag {
                "--config" => "BRIDGE_CONFIG",
                "--dry-run" => "BRIDGE_DRY_RUN",
                "--single-writer" => "BRIDGE_SINGLE_WRITER",
                "--health-probe-bind-address" => "BRIDGE_HEALTH_PROBE_BIND_ADDRESS",
                "--once" => "BRIDGE_ONCE",
                _ => bail!("unsupported option: {flag}"),
            };
            values.insert(key.into(), value.into());
        }
        let boolean = |key: &str| -> Result<bool> {
            values[key]
                .parse()
                .with_context(|| format!("{key} must be true or false"))
        };
        let config = values["BRIDGE_CONFIG"].clone();
        anyhow::ensure!(!config.is_empty(), "BRIDGE_CONFIG cannot be empty");
        let mut health_address = values["BRIDGE_HEALTH_PROBE_BIND_ADDRESS"].clone();
        anyhow::ensure!(
            !health_address.is_empty(),
            "BRIDGE_HEALTH_PROBE_BIND_ADDRESS cannot be empty"
        );
        if health_address.starts_with(':') {
            health_address = format!("0.0.0.0{health_address}");
        }
        let settings = Self {
            config,
            dry_run: boolean("BRIDGE_DRY_RUN")?,
            single_writer: boolean("BRIDGE_SINGLE_WRITER")?,
            health_address,
            once: boolean("BRIDGE_ONCE")?,
        };
        anyhow::ensure!(
            settings.dry_run || settings.single_writer,
            "writer requires BRIDGE_SINGLE_WRITER=true (or --single-writer=true) and an exclusive single replica deployment"
        );
        anyhow::ensure!(
            !settings.once || settings.dry_run,
            "BRIDGE_ONCE/--once is read-only"
        );
        Ok(settings)
    }
}
