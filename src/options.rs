// SPDX-License-Identifier: Apache-2.0
use crate::route::{HeaderModifier, HttpRouteFilter};
use crate::{ANNOTATION_PREFIX, Result, reject};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
/// Supported suffixes after `ANNOTATION_PREFIX`.
const KEYS: [&str; 4] = [
    "https-redirect",
    "https-redirect-status-code",
    "request-headers",
    "response-headers",
];
pub struct Options {
    pub redirect: bool,
    pub status: u16,
    pub filters: Vec<HttpRouteFilter>,
}
pub fn parse(annotations: &BTreeMap<String, String>) -> Result<Options> {
    // Keys sharing the prefix are contiguous in a BTreeMap, so one range scan finds
    // them without building the full key strings for each lookup.
    let mut values: [Option<&str>; 4] = [None; 4];
    let mut unknown = Vec::new();
    for (key, value) in
        annotations.range::<str, _>((Bound::Included(ANNOTATION_PREFIX), Bound::Unbounded))
    {
        let Some(suffix) = key.strip_prefix(ANNOTATION_PREFIX) else {
            break;
        };
        match KEYS.iter().position(|k| *k == suffix) {
            Some(index) => values[index] = Some(value),
            None => unknown.push(key.as_str()),
        }
    }
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
    if let Some(v) = values[0] {
        match v {
            "true" => o.redirect = true,
            "false" => (),
            _ => {
                return Err(reject(&format!(
                    "{ANNOTATION_PREFIX}{} must be true or false",
                    KEYS[0]
                )));
            }
        }
    }
    if let Some(v) = values[1] {
        if !o.redirect {
            return Err(reject(&format!(
                "{ANNOTATION_PREFIX}{} requires https-redirect: true",
                KEYS[1]
            )));
        }
        o.status = match v {
            "301" => 301,
            "302" => 302,
            _ => {
                return Err(reject(&format!(
                    "{ANNOTATION_PREFIX}{} must be 301 or 302",
                    KEYS[1]
                )));
            }
        };
    }
    for (index, kind) in [(2, "RequestHeaderModifier"), (3, "ResponseHeaderModifier")] {
        if let Some(v) = values[index] {
            let err = || {
                reject(&format!(
                    "{ANNOTATION_PREFIX}{} must be a valid JSON header filter with set/add/remove fields",
                    KEYS[index]
                ))
            };
            let h: HeaderModifier = serde_json::from_str(v).map_err(|_| err())?;
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
            o.filters.push(if index == 2 {
                HttpRouteFilter {
                    request_header_modifier: Some(h),
                    request_redirect: None,
                    response_header_modifier: None,
                    kind,
                }
            } else {
                HttpRouteFilter {
                    request_header_modifier: None,
                    request_redirect: None,
                    response_header_modifier: Some(h),
                    kind,
                }
            });
        }
    }
    Ok(o)
}
