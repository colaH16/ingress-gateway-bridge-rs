// SPDX-License-Identifier: Apache-2.0
//! Only fields consumed by translation are modeled; Kubernetes extensions are ignored.
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub ingress: Ingress,
    pub binding: Binding,
    #[serde(default)]
    pub services: Vec<Service>,
    #[serde(default = "default_controller")]
    pub controller_name: String,
}
pub fn default_controller() -> String {
    "ingress-gateway-bridge.colah16.github.io/controller".into()
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Metadata {
    pub name: String,
    pub namespace: String,
    pub uid: String,
    pub annotations: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Ingress {
    #[serde(default)]
    pub metadata: Metadata,
    pub spec: IngressSpec,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct IngressSpec {
    pub ingress_class_name: Option<String>,
    pub tls: Vec<Value>,
    pub default_backend: Option<Value>,
    pub rules: Vec<Rule>,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Rule {
    pub host: String,
    pub http: Option<Http>,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Http {
    pub paths: Vec<Path>,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Path {
    pub path: String,
    pub path_type: Option<String>,
    pub backend: Backend,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Backend {
    pub resource: Option<Value>,
    pub service: Option<ServiceBackend>,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ServiceBackend {
    pub name: String,
    pub port: Port,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Port {
    pub name: String,
    pub number: i32,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    #[serde(default)]
    pub http_redirect_parent_refs: Vec<Parent>,
    pub parent_refs: Vec<Parent>,
    #[serde(default)]
    pub tls_policy: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Parent {
    pub name: String,
    pub namespace: String,
    #[serde(default)]
    pub section_name: String,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Service {
    pub metadata: Metadata,
    pub spec: ServiceSpec,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ServiceSpec {
    pub ports: Vec<ServicePort>,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ServicePort {
    pub name: String,
    pub port: i32,
    pub protocol: String,
}
