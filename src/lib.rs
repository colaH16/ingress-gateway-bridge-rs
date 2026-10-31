// SPDX-License-Identifier: Apache-2.0
pub mod controller;
pub mod model;
pub mod options;
pub mod route;
pub mod settings;
use model::*;
use route::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const ANNOTATION_PREFIX: &str = "ingress-gateway-bridge.colah16.github.io/";
/// `ANNOTATION_PREFIX` + suffix, spelled out so lookups do not allocate.
pub const CONTROLLER_ANNOTATION: &str = "ingress-gateway-bridge.colah16.github.io/controller";
pub const SOURCE_ANNOTATION: &str = "ingress-gateway-bridge.colah16.github.io/source";
pub const REDIRECT_ANNOTATION: &str = "ingress-gateway-bridge.colah16.github.io/https-redirect";
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
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hash = Sha256::new();
    hash.update(name.as_bytes());
    hash.update([0]);
    hash.update(host.as_bytes());
    let digest = hash.finalize();
    // Kubernetes names are ASCII. floor_char_boundary is avoided to support older compilers.
    let mut end = name.len().min(39);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let stem = name[..end].trim_end_matches(['-', '.']);
    // One exact-size buffer instead of a String per hex byte plus format!.
    let mut result = String::with_capacity(stem.len() + "-bridge-".len() + 16);
    result.push_str(stem);
    result.push_str("-bridge-");
    for byte in &digest[..8] {
        result.push(HEX[usize::from(byte >> 4)] as char);
        result.push(HEX[usize::from(byte & 15)] as char);
    }
    result
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
fn backend_ref<'a>(
    namespace: &str,
    backend: &'a Backend,
    services: &[Service],
) -> Result<BackendRef<'a>> {
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
        let mut candidates = service
            .spec
            .ports
            .iter()
            .filter(|p| p.port == port)
            .peekable();
        if candidates.peek().is_some()
            && !candidates.any(|p| p.protocol.is_empty() || p.protocol == "TCP")
        {
            return Err(reject("HTTPRoute requires a TCP Service port"));
        }
    }
    Ok(BackendRef {
        group: "",
        kind: "Service",
        name: &svc.name,
        port,
        weight: 1,
    })
}

/// Pure conversion: borrows input, owns output, performs no I/O.
pub fn convert(input: &Input) -> Result<Vec<HttpRoute<'_>>> {
    translate(
        &input.ingress,
        &input.binding,
        &input.services,
        &input.controller_name,
    )
}

/// Same as `convert` with separately owned parts, like Go's `Convert` arguments.
/// The output borrows input strings; serialize or convert it before the input is dropped.
pub fn translate<'a>(
    ingress: &'a Ingress,
    binding: &'a Binding,
    services: &[Service],
    controller_name: &'a str,
) -> Result<Vec<HttpRoute<'a>>> {
    if ingress.metadata.uid.is_empty() {
        return Err(reject("source Ingress must have a UID"));
    }
    let options = options::parse(&ingress.metadata.annotations)?;
    if options.redirect
        && (binding.http_redirect_parent_refs.is_empty()
            || ingress.spec.tls.is_empty()
            || binding.tls_policy != "External")
    {
        return Err(reject(
            "HTTPS redirect requires httpRedirectParentRefs, Ingress TLS, and tlsPolicy: External",
        ));
    }
    if !ingress.spec.tls.is_empty() && binding.tls_policy != "External" {
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
    let mut rules: BTreeMap<&str, Vec<HttpRouteRule>> = BTreeMap::new();
    // Service name and port; the port is None while a named port is unresolved.
    let mut seen: BTreeMap<(&str, &str, &str), (&str, Option<i32>)> = BTreeMap::new();
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
            let backend = match backend_ref(&ingress.metadata.namespace, &path.backend, services) {
                Ok(backend) => Some(backend),
                Err(e @ Error::Rejected(_)) => return Err(e),
                Err(e) => {
                    if dependency_error.is_none() {
                        dependency_error = Some(e);
                    }
                    None
                }
            };
            let target = match &backend {
                Some(b) => (b.name, Some(b.port)),
                None => (
                    path.backend
                        .service
                        .as_ref()
                        .expect("validated backend")
                        .name
                        .as_str(),
                    None,
                ),
            };
            let key = (rule.host.as_str(), kind, value);
            if let Some(previous) = seen.get_mut(&key) {
                if previous.0 != target.0
                    || matches!((previous.1, target.1), (Some(a), Some(b)) if a != b)
                {
                    return Err(reject("duplicate host/path points to different backends"));
                }
                if previous.1.is_none() {
                    *previous = target;
                }
                continue;
            }
            seen.insert(key, target);
            let Some(backend) = backend else {
                continue;
            };
            rules.entry(&rule.host).or_default().push(HttpRouteRule {
                backend_refs: vec![backend],
                filters: options.filters.clone(),
                matches: vec![HttpRouteMatch {
                    path: PathMatch { kind, value },
                }],
            });
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
    let m = &ingress.metadata;
    let source = [m.namespace.as_str(), "/", m.name.as_str()].concat();
    let mut routes = Vec::with_capacity(rules.len() * if options.redirect { 2 } else { 1 });
    for (host, host_rules) in rules {
        if host_rules.len() > 16 {
            return Err(reject(
                "a host exceeds the Gateway API limit of 16 rules per HTTPRoute",
            ));
        }
        let hostnames = if host.is_empty() { vec![] } else { vec![host] };
        let metadata = RouteMetadata {
            annotations: Annotations {
                controller: controller_name,
                source: source.clone(),
            },
            labels: Labels {
                managed_by: "ingress-gateway-bridge",
            },
            name: route_name(&m.name, host),
            namespace: &m.namespace,
            owner_references: [OwnerReference {
                api_version: "networking.k8s.io/v1",
                block_owner_deletion: false,
                controller: true,
                kind: "Ingress",
                name: &m.name,
                uid: &m.uid,
            }],
        };
        let redirect = options.redirect.then(|| HttpRoute {
            api_version: API_VERSION,
            kind: "HTTPRoute",
            metadata: RouteMetadata {
                name: route_name(&m.name, &format!("{host}\0https-redirect")),
                ..metadata.clone()
            },
            spec: RouteSpec {
                hostnames: hostnames.clone(),
                parent_refs: parent_refs(&binding.http_redirect_parent_refs),
                rules: vec![HttpRouteRule {
                    backend_refs: vec![],
                    filters: vec![HttpRouteFilter {
                        request_header_modifier: None,
                        request_redirect: Some(RequestRedirect {
                            port: 443,
                            scheme: "https",
                            status_code: options.status,
                        }),
                        response_header_modifier: None,
                        kind: "RequestRedirect",
                    }],
                    matches: vec![HttpRouteMatch {
                        path: PathMatch {
                            kind: "PathPrefix",
                            value: "/",
                        },
                    }],
                }],
            },
        });
        routes.push(HttpRoute {
            api_version: API_VERSION,
            kind: "HTTPRoute",
            metadata,
            spec: RouteSpec {
                hostnames,
                parent_refs: parent_refs(&binding.parent_refs),
                rules: host_rules,
            },
        });
        routes.extend(redirect);
    }
    Ok(routes)
}

pub fn parent_refs(parents: &[Parent]) -> Vec<ParentReference<'_>> {
    parents
        .iter()
        .map(|p| ParentReference {
            group: GATEWAY_GROUP,
            kind: "Gateway",
            name: &p.name,
            namespace: &p.namespace,
            section_name: &p.section_name,
        })
        .collect()
}
