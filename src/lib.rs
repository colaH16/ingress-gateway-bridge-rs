// SPDX-License-Identifier: Apache-2.0
pub mod controller;
pub mod model;
pub mod options;
use model::*;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const ANNOTATION_PREFIX: &str = "ingress-gateway-bridge.colah16.github.io/";
const LEGACY_CLASS: &str = "kubernetes.io/ingress.class";

/// Rejected is permanent; Dependency means a Service snapshot may resolve it later.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum Error {
    Rejected(String),
    Dependency(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(s) | Self::Dependency(s) => f.write_str(s),
        }
    }
}
impl std::error::Error for Error {}
fn reject(s: &str) -> Error {
    Error::Rejected(s.into())
}
type Result<T> = std::result::Result<T, Error>;

/// Borrow the class string from the source instead of allocating a copy.
pub fn class_name(ingress: &Ingress) -> Result<&str> {
    let legacy = ingress
        .metadata
        .annotations
        .get(LEGACY_CLASS)
        .map(String::as_str)
        .unwrap_or("");
    match ingress.spec.ingress_class_name.as_deref() {
        Some(name) if !legacy.is_empty() && legacy != name => Err(reject(
            "ingressClassName conflicts with the legacy class annotation",
        )),
        Some(name) => Ok(name),
        None => Ok(legacy),
    }
}
fn label(s: &str, limit: usize, alpha_first: bool) -> bool {
    !s.is_empty()
        && s.len() <= limit
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && (!alpha_first || s.as_bytes()[0].is_ascii_lowercase())
        && s.as_bytes()[s.len() - 1].is_ascii_alphanumeric()
}
pub fn dns(s: &str) -> bool {
    !s.is_empty() && s.len() <= 253 && s.split('.').all(|part| label(part, 253, false))
}
/// This CLI accepts a preselected binding. Selection/IngressClass ownership belongs to the controller.
pub fn validate_binding(binding: &Binding) -> std::result::Result<(), String> {
    if !(1..=16).contains(&binding.parent_refs.len()) {
        return Err("binding must have 1..16 parentRefs".into());
    }
    if !["", "Reject", "External"].contains(&binding.tls_policy.as_str()) {
        return Err("tlsPolicy must be Reject or External".into());
    }
    if binding.http_redirect_parent_refs.len() > 16 {
        return Err("at most 16 httpRedirectParentRefs".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for p in binding
        .parent_refs
        .iter()
        .chain(&binding.http_redirect_parent_refs)
    {
        if !dns(&p.name)
            || !label(&p.namespace, 63, false)
            || (!p.section_name.is_empty() && !dns(&p.section_name))
        {
            return Err("invalid Gateway parent reference".into());
        }
        if !seen.insert((&p.name, &p.namespace, &p.section_name)) {
            return Err("duplicate Gateway parent reference".into());
        }
        if !binding.http_redirect_parent_refs.is_empty() && p.section_name.is_empty() {
            return Err("redirect bindings require explicit sectionName".into());
        }
    }
    for parents in [&binding.parent_refs, &binding.http_redirect_parent_refs] {
        let mut scoped = BTreeMap::new();
        for p in parents {
            if let Some(old) = scoped.insert((&p.name, &p.namespace), !p.section_name.is_empty())
                && old == p.section_name.is_empty()
            {
                return Err(
                    "references to one Gateway must all set or all omit sectionName".into(),
                );
            }
        }
    }

    Ok(())
}

pub fn route_name(name: &str, host: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(name.as_bytes());
    hash.update([0]);
    hash.update(host.as_bytes());
    let suffix: String = hash.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    // Kubernetes names are ASCII. floor_char_boundary is avoided to support older compilers.
    let mut end = name.len().min(39);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}-bridge-{suffix}",
        name[..end].trim_end_matches(['-', '.'])
    )
}

fn literal_path(s: &str) -> bool {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN
        .get_or_init(|| {
            regex::Regex::new(r"^(?:[-A-Za-z0-9/._~!$&'()*+,;=:@]|[%][0-9a-fA-F]{2})+$")
                .expect("static regex")
        })
        .is_match(s)
}
fn path_match(path: &Path) -> Result<(&'static str, &str)> {
    let kind = match path.path_type.as_deref() {
        None => return Err(reject("pathType is required")),
        Some("Exact") => "Exact",
        Some("Prefix") => "PathPrefix",
        _ => {
            return Err(reject(
                "ImplementationSpecific paths are not supported yet; use Prefix or Exact",
            ));
        }
    };
    let s = path.path.as_str();
    if !s.starts_with('/')
        || ["//", "%2f", "%2F", "?", "#", "/../", "/./"]
            .iter()
            .any(|x| s.contains(x))
        || s.ends_with("/..")
        || s.ends_with("/.")
        || s.len() > 1024
        || !literal_path(s)
    {
        return Err(reject(
            "path cannot be represented as a Gateway API literal path",
        ));
    }
    Ok((
        kind,
        if kind == "PathPrefix" && s != "/" {
            s.trim_end_matches('/')
        } else {
            s
        },
    ))
}
fn backend_ref(namespace: &str, backend: &Backend, services: &[Service]) -> Result<Value> {
    let svc = backend
        .service
        .as_ref()
        .filter(|_| backend.resource.is_none())
        .ok_or_else(|| reject("only Service backends are supported"))?;
    if !label(&svc.name, 63, true) {
        return Err(reject("invalid backend Service name"));
    }
    if svc.port.name.is_empty() == (svc.port.number == 0) {
        return Err(reject(
            "backend must have exactly one numeric or named Service port",
        ));
    }
    // Last snapshot wins, matching the Go map populated by the comparison adapter.
    let service = services
        .iter()
        .rev()
        .find(|s| s.metadata.namespace == namespace && s.metadata.name == svc.name);
    let mut port = svc.port.number;
    if !svc.port.name.is_empty() {
        let service = service.ok_or_else(|| {
            Error::Dependency(format!(
                "named-port backend Service {namespace}/{} is not available",
                svc.name
            ))
        })?;
        if let Some(candidate) = service.spec.ports.iter().find(|p| p.name == svc.port.name) {
            if !candidate.protocol.is_empty() && candidate.protocol != "TCP" {
                return Err(reject("HTTPRoute requires a TCP Service port"));
            }
            port = candidate.port;
        }
        if port == 0 {
            return Err(Error::Dependency(format!(
                "named Service port is not available for {namespace}/{}",
                svc.name
            )));
        }
    }
    if !(1..=65535).contains(&port) {
        return Err(reject("backend Service port must be 1..65535"));
    }
    if let Some(service) = service {
        let candidates: Vec<_> = service
            .spec
            .ports
            .iter()
            .filter(|p| p.port == port)
            .collect();
        if !candidates.is_empty()
            && !candidates
                .iter()
                .any(|p| p.protocol.is_empty() || p.protocol == "TCP")
        {
            return Err(reject("HTTPRoute requires a TCP Service port"));
        }
    }
    Ok(json!({"name":svc.name,"port":port,"group":"","kind":"Service","weight":1}))
}

/// Pure conversion: borrows input, owns output, performs no I/O.
pub fn convert(input: &Input) -> Result<Vec<Value>> {
    let ingress = &input.ingress;
    if ingress.metadata.uid.is_empty() {
        return Err(reject("source Ingress must have a UID"));
    }
    let options = options::parse(&ingress.metadata.annotations)?;
    if options.redirect
        && (input.binding.http_redirect_parent_refs.is_empty()
            || ingress.spec.tls.is_empty()
            || input.binding.tls_policy != "External")
    {
        return Err(reject(
            "HTTPS redirect requires httpRedirectParentRefs, Ingress TLS, and tlsPolicy: External",
        ));
    }
    if !ingress.spec.tls.is_empty() && input.binding.tls_policy != "External" {
        return Err(reject(
            "Ingress TLS requires tlsPolicy: External and preconfigured TLS; secrets are not copied",
        ));
    }
    if ingress.spec.default_backend.is_some() {
        return Err(reject(
            "defaultBackend is not supported yet; configure an explicit host/path rule",
        ));
    }
    if ingress.spec.rules.is_empty() {
        return Err(reject("at least one HTTP rule is required"));
    }
    let mut rules: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    let mut seen: BTreeMap<(&str, &str, &str), Value> = BTreeMap::new();
    let mut dependency_error = None;
    for rule in &ingress.spec.rules {
        if rule.host.contains('*') {
            return Err(reject(
                "wildcard Ingress hosts have different matching semantics from Gateway API and are not supported yet",
            ));
        }
        if !rule.host.is_empty() && !dns(&rule.host) {
            return Err(reject("invalid host in Ingress"));
        }
        let http = rule
            .http
            .as_ref()
            .filter(|h| !h.paths.is_empty())
            .ok_or_else(|| reject("every rule must contain HTTP paths"))?;
        for path in &http.paths {
            let (kind, value) = path_match(path)?;
            let backend = match backend_ref(
                &ingress.metadata.namespace,
                &path.backend,
                &input.services,
            ) {
                Ok(value) => value,
                Err(e @ Error::Rejected(_)) => return Err(e),
                Err(e) => {
                    if dependency_error.is_none() {
                        dependency_error = Some(e);
                    }
                    json!({"name":path.backend.service.as_ref().expect("validated backend").name})
                }
            };
            let key = (rule.host.as_str(), kind, value);
            if let Some(previous) = seen.get(&key) {
                if previous["name"] != backend["name"]
                    || (!previous["port"].is_null()
                        && !backend["port"].is_null()
                        && previous["port"] != backend["port"])
                {
                    return Err(reject("duplicate host/path points to different backends"));
                }
                if previous["port"].is_null() {
                    seen.insert(key, backend);
                }
                continue;
            }
            seen.insert(key, backend.clone());
            if backend["port"].is_null() {
                continue;
            }
            rules.entry(&rule.host).or_default().push(
                json!({"matches":[{"path":{"type":kind,"value":value}}],"backendRefs":[backend]}),
            );
        }
    }
    let mut counts = BTreeMap::new();
    for (host, _, _) in seen.keys() {
        *counts.entry(host).or_insert(0) += 1;
    }
    if counts.values().any(|n| *n > 16) {
        return Err(reject(
            "a host exceeds the Gateway API limit of 16 rules per HTTPRoute",
        ));
    }
    if let Some(e) = dependency_error {
        return Err(e);
    }
    let parents: Vec<_> = input.binding.parent_refs.iter().map(|p| {
        let mut v = json!({"name":p.name,"namespace":p.namespace,"group":"gateway.networking.k8s.io","kind":"Gateway"});
        if !p.section_name.is_empty() { v["sectionName"] = json!(p.section_name); } v
    }).collect();
    let mut routes = Vec::new();
    for (host, mut host_rules) in rules {
        if !options.filters.is_empty() {
            for rule in &mut host_rules {
                rule["filters"] = json!(options.filters);
            }
        }
        if host_rules.len() > 16 {
            return Err(reject(
                "a host exceeds the Gateway API limit of 16 rules per HTTPRoute",
            ));
        }
        let m = &ingress.metadata;
        let mut spec = json!({"parentRefs":parents,"rules":host_rules});
        if !host.is_empty() {
            spec["hostnames"] = json!([host]);
        }
        routes.push(json!({"apiVersion":"gateway.networking.k8s.io/v1","kind":"HTTPRoute",
            "metadata":{"name":route_name(&m.name,host),"namespace":m.namespace,
                "labels":{"app.kubernetes.io/managed-by":"ingress-gateway-bridge"},
                "annotations":{format!("{ANNOTATION_PREFIX}controller"):input.controller_name,format!("{ANNOTATION_PREFIX}source"):format!("{}/{}",m.namespace,m.name)},
                "ownerReferences":[{"apiVersion":"networking.k8s.io/v1","kind":"Ingress","name":m.name,"uid":m.uid,"controller":true,"blockOwnerDeletion":false}]},
            "spec":spec}));
    }
    if options.redirect {
        let mut result = Vec::new();
        for route in routes {
            let host = route["spec"]["hostnames"][0].as_str().unwrap_or("");
            let mut redirect = route.clone();
            redirect["metadata"]["name"] = json!(route_name(
                &ingress.metadata.name,
                &format!("{host}\0https-redirect")
            ));
            redirect["spec"]["parentRefs"] =
                json!(parent_refs(&input.binding.http_redirect_parent_refs));
            redirect["spec"]["rules"] = json!([{"matches":[{"path":{"type":"PathPrefix","value":"/"}}],"filters":[{"type":"RequestRedirect","requestRedirect":{"scheme":"https","port":443,"statusCode":options.status}}]}]);
            result.push(route);
            result.push(redirect);
        }
        return Ok(result);
    }
    Ok(routes)
}

pub fn parent_refs(parents: &[Parent]) -> Vec<Value> {
    parents.iter().map(|p| { let mut v=json!({"name":p.name,"namespace":p.namespace,"group":"gateway.networking.k8s.io","kind":"Gateway"}); if !p.section_name.is_empty() { v["sectionName"]=json!(p.section_name); } v }).collect()
}
