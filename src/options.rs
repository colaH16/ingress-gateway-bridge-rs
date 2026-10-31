// SPDX-License-Identifier: Apache-2.0
use crate::{ANNOTATION_PREFIX, Result, reject};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Header {
    name: String,
    value: String,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Headers {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    set: Vec<Header>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    add: Vec<Header>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    remove: Vec<String>,
}
pub struct Options {
    pub redirect: bool,
    pub status: u16,
    pub filters: Vec<Value>,
}
pub fn parse(annotations: &BTreeMap<String, String>) -> Result<Options> {
    let keys = [
        "https-redirect",
        "https-redirect-status-code",
        "request-headers",
        "response-headers",
    ]
    .map(|k| format!("{ANNOTATION_PREFIX}{k}"));
    let unknown: Vec<_> = annotations
        .keys()
        .filter(|k| k.starts_with(ANNOTATION_PREFIX) && !keys.contains(k))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(reject(&format!(
            "unknown bridge Ingress annotations: {}",
            unknown.join(", ")
        )));
    }
    let mut o = Options {
        redirect: false,
        status: 302,
        filters: Vec::new(),
    };
    if let Some(v) = annotations.get(&keys[0]) {
        match v.as_str() {
            "true" => o.redirect = true,
            "false" => (),
            _ => return Err(reject(&format!("{} must be true or false", keys[0]))),
        }
    }
    if let Some(v) = annotations.get(&keys[1]) {
        if !o.redirect {
            return Err(reject(&format!(
                "{} requires https-redirect: true",
                keys[1]
            )));
        }
        o.status = match v.as_str() {
            "301" => 301,
            "302" => 302,
            _ => return Err(reject(&format!("{} must be 301 or 302", keys[1]))),
        };
    }
    for (key, kind, field) in [
        (&keys[2], "RequestHeaderModifier", "requestHeaderModifier"),
        (&keys[3], "ResponseHeaderModifier", "responseHeaderModifier"),
    ] {
        if let Some(v) = annotations.get(key) {
            let err = || {
                reject(&format!(
                    "{key} must be a valid JSON header filter with set/add/remove fields"
                ))
            };
            let h: Headers = serde_json::from_str(v).map_err(|_| err())?;
            if h.set.len() > 16
                || h.add.len() > 16
                || h.remove.len() > 16
                || h.set.len() + h.add.len() + h.remove.len() == 0
            {
                return Err(err());
            }
            let mut seen = BTreeSet::new();
            for name in h.set.iter().chain(&h.add).map(|h| &h.name).chain(&h.remove) {
                if name.is_empty()
                    || name.len() > 256
                    || !name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c))
                    || !seen.insert(name.to_ascii_lowercase())
                {
                    return Err(err());
                }
            }
            for header in h.set.iter().chain(&h.add) {
                if header.value.len() > 4096
                    || header.value.bytes().any(|b| b == 127 || (b < 32 && b != 9))
                {
                    return Err(err());
                }
            }
            o.filters.push(json!({"type":kind,field:h}));
        }
    }
    Ok(o)
}
