// SPDX-License-Identifier: Apache-2.0
//! Typed HTTPRoute output, limited to the fields this translator generates.
//!
//! Strings copied verbatim from the input are borrowed (`&'a str`), like Go strings
//! that share their backing bytes; only computed strings (route name, source) are owned.
//! Fields are declared in alphabetical JSON-name order, so the serialized bytes equal the
//! former `serde_json::Value` (BTreeMap, sorted keys) output. `tests/translation.rs` checks it.
use serde::{Deserialize, Serialize};

pub const API_VERSION: &str = "gateway.networking.k8s.io/v1";
pub const GATEWAY_GROUP: &str = "gateway.networking.k8s.io";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRoute<'a> {
    pub api_version: &'static str,
    pub kind: &'static str,
    pub metadata: RouteMetadata<'a>,
    pub spec: RouteSpec<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteMetadata<'a> {
    pub annotations: Annotations<'a>,
    pub labels: Labels,
    pub name: String,
    pub namespace: &'a str,
    pub owner_references: [OwnerReference<'a>; 1],
}

/// serde `rename` needs literals; a test checks these match `ANNOTATION_PREFIX`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Annotations<'a> {
    #[serde(rename = "ingress-gateway-bridge.colah16.github.io/controller")]
    pub controller: &'a str,
    #[serde(rename = "ingress-gateway-bridge.colah16.github.io/source")]
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Labels {
    #[serde(rename = "app.kubernetes.io/managed-by")]
    pub managed_by: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerReference<'a> {
    pub api_version: &'static str,
    pub block_owner_deletion: bool,
    pub controller: bool,
    pub kind: &'static str,
    pub name: &'a str,
    pub uid: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteSpec<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hostnames: Vec<&'a str>,
    pub parent_refs: Vec<ParentReference<'a>>,
    pub rules: Vec<HttpRouteRule<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentReference<'a> {
    pub group: &'static str,
    pub kind: &'static str,
    pub name: &'a str,
    pub namespace: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub section_name: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRouteRule<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub backend_refs: Vec<BackendRef<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<HttpRouteFilter>,
    pub matches: Vec<HttpRouteMatch<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackendRef<'a> {
    pub group: &'static str,
    pub kind: &'static str,
    pub name: &'a str,
    pub port: i32,
    pub weight: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HttpRouteMatch<'a> {
    pub path: PathMatch<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathMatch<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub value: &'a str,
}

/// Exactly one modifier is set, matching `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRouteFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_header_modifier: Option<HeaderModifier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_redirect: Option<RequestRedirect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_header_modifier: Option<HeaderModifier>,
    #[serde(rename = "type")]
    pub kind: &'static str,
}

/// Parsed from the header annotations and emitted unchanged, minus empty lists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderModifier {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add: Vec<Header>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set: Vec<Header>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRedirect {
    pub port: u16,
    pub scheme: &'static str,
    pub status_code: u16,
}
