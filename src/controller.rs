// SPDX-License-Identifier: Apache-2.0
//! A serial, single-writer reconciler. Watches trigger a debounced resync;
//! periodic resync also retries dependency failures and recovers missed events.
use crate::{
    ANNOTATION_PREFIX, Error, class_name, convert, dns,
    model::{Binding, Input},
    validate_binding,
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    #[serde(default = "crate::model::default_controller")]
    pub controller_name: String,
    pub classes: BTreeMap<String, Binding>,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        let (domain, name) = self
            .controller_name
            .split_once('/')
            .context("controllerName must be domain-qualified")?;
        anyhow::ensure!(
            dns(domain)
                && !name.is_empty()
                && name.len() <= 63
                && name.starts_with(|c: char| c.is_ascii_alphanumeric())
                && name.ends_with(|c: char| c.is_ascii_alphanumeric())
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)),
            "invalid controllerName"
        );
        anyhow::ensure!(
            !self.classes.is_empty(),
            "at least one class binding is required"
        );
        for (class, binding) in &self.classes {
            anyhow::ensure!(dns(class), "invalid class name");
            validate_binding(binding).map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
}
#[derive(Default, Debug)]
pub struct Snapshot {
    pub ingresses: Vec<Value>,
    pub classes: Vec<Value>,
    pub services: Vec<Value>,
    pub routes: Vec<Value>,
    pub gateways: Vec<Value>,
    pub namespaces: Vec<Value>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Create(Value),
    Update(Value),
    Delete(Value),
}
impl Action {
    pub fn object(&self) -> &Value {
        match self {
            Self::Create(v) | Self::Update(v) | Self::Delete(v) => v,
        }
    }
    pub fn verb(&self) -> &str {
        match self {
            Self::Create(_) => "create",
            Self::Update(_) => "update",
            Self::Delete(_) => "delete",
        }
    }
}
pub fn name(v: &Value) -> &str {
    v["metadata"]["name"].as_str().unwrap_or("")
}
pub fn namespace(v: &Value) -> &str {
    v["metadata"]["namespace"].as_str().unwrap_or("")
}
pub fn owns(route: &Value, ingress: &Value, controller: &str) -> bool {
    let owners = route["metadata"]["ownerReferences"].as_array();
    owners
        .and_then(|o| o.iter().find(|o| o["controller"] == true))
        .is_some_and(|o| {
            o["apiVersion"] == "networking.k8s.io/v1"
                && o["kind"] == "Ingress"
                && o["name"] == name(ingress)
                && !ingress["metadata"]["uid"].is_null()
                && o["uid"] == ingress["metadata"]["uid"]
        })
        && route["metadata"]["annotations"][format!("{ANNOTATION_PREFIX}controller")] == controller
        && namespace(route) == namespace(ingress)
}
fn selector_matches(selector: &Value, labels: &Value) -> bool {
    if let Some(map) = selector["matchLabels"].as_object() {
        for (k, v) in map {
            if labels.get(k) != Some(v) {
                return false;
            }
        }
    }
    if let Some(exprs) = selector["matchExpressions"].as_array() {
        for e in exprs {
            let Some(key) = e["key"].as_str() else {
                return false;
            };
            let value = labels.get(key);
            let values = e["values"].as_array();
            let contains = value.is_some_and(|v| values.is_some_and(|a| a.contains(v)));
            let matched = match e["operator"].as_str() {
                Some("In") => contains,
                Some("NotIn") => !contains,
                Some("Exists") => value.is_some(),
                Some("DoesNotExist") => value.is_none(),
                _ => false,
            };
            if !matched {
                return false;
            }
        }
    }
    true
}
fn check_parents(ingress: &Value, binding: &Binding, s: &Snapshot, redirect: bool) -> Result<()> {
    for (index, parent) in binding
        .parent_refs
        .iter()
        .chain(if redirect {
            binding.http_redirect_parent_refs.iter()
        } else {
            [].iter()
        })
        .enumerate()
    {
        let gateway = s
            .gateways
            .iter()
            .find(|g| name(g) == parent.name && namespace(g) == parent.namespace)
            .context("Gateway dependency unavailable")?;
        let listeners = gateway["spec"]["listeners"]
            .as_array()
            .context("Gateway listeners unavailable")?;
        let eligible = listeners.iter().any(|listener| {
            if !parent.section_name.is_empty() && listener["name"] != parent.section_name {
                return false;
            }
            if listener["protocol"] != "HTTP" && listener["protocol"] != "HTTPS" {
                return false;
            }
            if redirect {
                let forward = index < binding.parent_refs.len();
                if listener["protocol"] != if forward { "HTTPS" } else { "HTTP" }
                    || listener["port"] != if forward { 443 } else { 80 }
                {
                    return false;
                }
            }
            let allowed = &listener["allowedRoutes"];
            if let Some(kinds) = allowed["kinds"].as_array()
                && !kinds.is_empty()
                && !kinds.iter().any(|k| {
                    k["kind"] == "HTTPRoute"
                        && (k["group"].is_null() || k["group"] == "gateway.networking.k8s.io")
                })
            {
                return false;
            }
            match allowed["namespaces"]["from"].as_str().unwrap_or("Same") {
                "All" => true,
                "Same" => namespace(ingress) == namespace(gateway),
                "Selector" => {
                    let selector = &allowed["namespaces"]["selector"];
                    !selector.is_null()
                        && s.namespaces
                            .iter()
                            .find(|n| name(n) == namespace(ingress))
                            .is_some_and(|n| selector_matches(selector, &n["metadata"]["labels"]))
                }
                _ => false,
            }
        });
        anyhow::ensure!(
            eligible,
            "Gateway {}/{} has no selected HTTP(S) listener allowing namespace {}",
            parent.namespace,
            parent.name,
            namespace(ingress)
        );
    }
    Ok(())
}
/// No mutation until all translation, dependency, parent and collision checks pass.
pub fn plan(ingress: &Value, cfg: &Config, s: &Snapshot) -> Result<Vec<Action>> {
    if !ingress["metadata"]["deletionTimestamp"].is_null() {
        return Ok(vec![]);
    }
    let owned: Vec<_> = s
        .routes
        .iter()
        .filter(|r| owns(r, ingress, &cfg.controller_name))
        .collect();
    let remove = || owned.iter().map(|r| Action::Delete((*r).clone())).collect();
    let parsed: crate::model::Ingress = serde_json::from_value(ingress.clone())?;
    let class = match class_name(&parsed) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(ingress=name(ingress),reason=%e,"class conflict");
            return Ok(remove());
        }
    };
    let Some(binding) = cfg.classes.get(class) else {
        return Ok(remove());
    };
    if !s
        .classes
        .iter()
        .any(|c| name(c) == class && c["spec"]["controller"] == cfg.controller_name)
    {
        return Ok(remove());
    }
    let services = s
        .services
        .iter()
        .filter(|v| namespace(v) == namespace(ingress))
        .map(|v| serde_json::from_value(v.clone()))
        .collect::<std::result::Result<_, _>>()?;
    let desired = match convert(&Input {
        ingress: parsed,
        binding: binding.clone(),
        services,
        controller_name: cfg.controller_name.clone(),
    }) {
        Ok(routes) => routes,
        Err(Error::Rejected(reason)) => {
            tracing::warn!(ingress=name(ingress),%reason,"source rejected; withdrawing owned routes");
            return Ok(remove());
        }
        Err(e) => return Err(e.into()),
    };
    let redirect =
        ingress["metadata"]["annotations"][format!("{ANNOTATION_PREFIX}https-redirect")] == "true";
    check_parents(ingress, binding, s, redirect)?;
    let mut actions = Vec::new();
    let mut wanted = BTreeSet::new();
    for route in desired {
        wanted.insert(name(&route).to_string());
        if let Some(previous) = s
            .routes
            .iter()
            .find(|r| name(r) == name(&route) && namespace(r) == namespace(&route))
        {
            anyhow::ensure!(
                owns(previous, ingress, &cfg.controller_name),
                "HTTPRoute name collision: {}/{}",
                namespace(&route),
                name(&route)
            );
            let mut next = previous.clone();
            next["spec"] = route["spec"].clone();
            for field in ["annotations", "labels"] {
                if !next["metadata"][field].is_object() {
                    next["metadata"][field] = json!({});
                }
                for (key, value) in route["metadata"][field]
                    .as_object()
                    .context("generated metadata")?
                {
                    next["metadata"][field][key] = value.clone();
                }
            }
            if previous["spec"] != next["spec"]
                || previous["metadata"]["labels"] != next["metadata"]["labels"]
                || previous["metadata"]["annotations"] != next["metadata"]["annotations"]
            {
                actions.push(Action::Update(next));
            }
        } else {
            actions.push(Action::Create(route));
        }
    }
    for route in owned {
        if !wanted.contains(name(route)) {
            actions.push(Action::Delete(route.clone()));
        }
    }
    Ok(actions)
}

pub mod runtime {
    use super::*;
    use futures::{StreamExt, pin_mut};
    use kube::{
        Api, Client, ResourceExt,
        api::{
            ApiResource, DeleteParams, DynamicObject, GroupVersionKind, ListParams, PostParams,
            Preconditions,
        },
        runtime::{WatchStreamExt, watcher},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    use tokio::{
        sync::Notify,
        time::{Duration, Instant},
    };
    pub const KINDS: [(&str, &str, &str); 6] = [
        ("networking.k8s.io", "v1", "Ingress"),
        ("networking.k8s.io", "v1", "IngressClass"),
        ("", "v1", "Service"),
        ("gateway.networking.k8s.io", "v1", "HTTPRoute"),
        ("gateway.networking.k8s.io", "v1", "Gateway"),
        ("", "v1", "Namespace"),
    ];
    pub fn api(client: Client, index: usize, ns: Option<&str>) -> Api<DynamicObject> {
        let (group, version, kind) = KINDS[index];
        let ar = ApiResource::from_gvk(&GroupVersionKind::gvk(group, version, kind));
        match ns {
            Some(ns) => Api::namespaced_with(client, ns, &ar),
            None => Api::all_with(client, &ar),
        }
    }
    async fn list(client: Client, index: usize) -> Result<Vec<Value>> {
        let api = api(client, index, None);
        let mut result = Vec::new();
        let mut lp = ListParams::default().limit(500);
        loop {
            let page = api.list(&lp).await?;
            for item in page.items {
                result.push(serde_json::to_value(item)?);
            }
            match page.metadata.continue_.filter(|s| !s.is_empty()) {
                Some(token) => lp = lp.continue_token(&token),
                None => break,
            }
        }
        Ok(result)
    }
    pub async fn snapshot(client: Client) -> Result<Snapshot> {
        let (ingresses, classes, services, routes, gateways, namespaces) = tokio::try_join!(
            list(client.clone(), 0),
            list(client.clone(), 1),
            list(client.clone(), 2),
            list(client.clone(), 3),
            list(client.clone(), 4),
            list(client, 5)
        )?;
        Ok(Snapshot {
            ingresses,
            classes,
            services,
            routes,
            gateways,
            namespaces,
        })
    }
    async fn apply(client: Client, ingress: &Value, actions: &[Action]) -> Result<()> {
        for action in actions {
            // Fail/replan if the source changed since this snapshot. Route updates carry RV,
            // deletes carry both UID and RV; concurrent ownership changes cannot be overwritten.
            let source = api(client.clone(), 0, Some(namespace(ingress)))
                .get(name(ingress))
                .await?;
            anyhow::ensure!(
                source.uid().as_deref() == ingress["metadata"]["uid"].as_str()
                    && source.resource_version().as_deref()
                        == ingress["metadata"]["resourceVersion"].as_str()
                    && source.metadata.deletion_timestamp.is_none(),
                "source changed; replan required"
            );
            let v = action.object();
            let routes = api(client.clone(), 3, Some(namespace(v)));
            match action {
                Action::Create(v) => {
                    let object = serde_json::from_value(v.clone())?;
                    routes.create(&PostParams::default(), &object).await?;
                }
                Action::Update(v) => {
                    let object = serde_json::from_value(v.clone())?;
                    routes
                        .replace(name(v), &PostParams::default(), &object)
                        .await?;
                }
                Action::Delete(v) => {
                    let uid = v["metadata"]["uid"]
                        .as_str()
                        .context("delete requires UID")?
                        .to_string();
                    let rv = v["metadata"]["resourceVersion"]
                        .as_str()
                        .context("delete requires resourceVersion")?
                        .to_string();
                    let dp = DeleteParams {
                        preconditions: Some(Preconditions {
                            uid: Some(uid),
                            resource_version: Some(rv),
                        }),
                        ..Default::default()
                    };
                    match routes.delete(name(v), &dp).await {
                        Ok(_) => (),
                        Err(kube::Error::Api(e)) if e.code == 404 => (),
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }
        Ok(())
    }
    async fn bounded<T>(operation: impl std::future::Future<Output = Result<T>>) -> Result<T> {
        tokio::time::timeout(Duration::from_secs(45), operation)
            .await
            .context("Kubernetes operation exceeded 45 seconds")?
    }
    pub async fn run(cfg: Config, dry_run: bool, once: bool, health: &str) -> Result<()> {
        let mut kc = kube::Config::infer().await?;
        kc.connect_timeout = Some(Duration::from_secs(10));
        // kube watcher requests run for up to 290s; ordinary operations have
        // their own shorter deadline below. Do not interrupt healthy watch bodies.
        kc.read_timeout = Some(Duration::from_secs(330));
        let client = Client::try_from(kc)?;
        let start = Instant::now();
        let last = Arc::new(AtomicU64::new(0));
        let signal = Arc::new(Notify::new());
        let mut tasks = tokio::task::JoinSet::new();
        if !once {
            let listener = tokio::net::TcpListener::bind(health).await?;
            let last = last.clone();
            tasks.spawn(async move {
                use tokio::io::{AsyncReadExt,AsyncWriteExt};
                loop { let (mut socket,_)=listener.accept().await?; let last=last.clone(); tokio::spawn(async move {
                    let mut bytes=[0;1024]; let n=tokio::time::timeout(Duration::from_secs(2),socket.read(&mut bytes)).await.ok().and_then(|v|v.ok()).unwrap_or(0);
                    let request=String::from_utf8_lossy(&bytes[..n]); let tick=last.load(Ordering::Relaxed);
                    let live=tick>0 && start.elapsed().as_secs()+1-tick<180;
                    let ready=tick>0 && start.elapsed().as_secs()+1-tick<90;
                    let status=if request.starts_with("GET /healthz ") { if live || (tick==0 && start.elapsed().as_secs()<180) {"200 OK"} else {"503 Service Unavailable"} } else if request.starts_with("GET /readyz ") { if ready {"200 OK"} else {"503 Service Unavailable"} } else {"404 Not Found"};
                    let _=socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await;
                }); }
                #[allow(unreachable_code)] Ok::<(),anyhow::Error>(())
            });
            for (index, kind) in KINDS.iter().enumerate() {
                let api = api(client.clone(), index, None);
                let notify = signal.clone();
                tasks.spawn(async move {
                    let stream = watcher(api, watcher::Config::default()).default_backoff();
                    pin_mut!(stream);
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(_) => notify.notify_one(),
                            Err(e) => tracing::warn!(kind=kind.2,error=%e,"watch retry"),
                        }
                    }
                    bail!("watch stream ended")
                });
            }
        }
        let mut delay = Duration::from_secs(1);
        loop {
            let mut changes = 0;
            let mut errors = 0;
            match bounded(snapshot(client.clone())).await {
                Ok(s) => {
                    for ingress in &s.ingresses {
                        match plan(ingress, &cfg, &s) {
                            Ok(actions) => {
                                changes += actions.len();
                                for action in &actions {
                                    tracing::info!(
                                        action = action.verb(),
                                        namespace = namespace(action.object()),
                                        route = name(action.object()),
                                        dry_run,
                                        "HTTPRoute plan"
                                    );
                                }
                                if !dry_run
                                    && let Err(e) =
                                        bounded(apply(client.clone(), ingress, &actions)).await
                                {
                                    errors += 1;
                                    tracing::error!(namespace=namespace(ingress),ingress=name(ingress),error=%e,"apply failed; retry scheduled");
                                }
                            }
                            Err(e) => {
                                errors += 1;
                                tracing::warn!(namespace=namespace(ingress),ingress=name(ingress),error=%e,"reconciliation deferred");
                            }
                        }
                    }
                    last.store(start.elapsed().as_secs() + 1, Ordering::Relaxed);
                    tracing::info!(
                        sources = s.ingresses.len(),
                        routes = s.routes.len(),
                        changes,
                        errors,
                        dry_run,
                        "resync complete"
                    );
                    if once {
                        anyhow::ensure!(errors == 0, "{errors} sources failed reconciliation");
                        return Ok(());
                    }
                }
                Err(e) => {
                    if once {
                        return Err(e);
                    }
                    errors += 1;
                    tracing::error!(error=%e,"snapshot failed; preserving routes");
                }
            }
            delay = if errors > 0 {
                (delay * 2).min(Duration::from_secs(30))
            } else {
                Duration::from_secs(1)
            };
            // Coalesce watch bursts and bound retry load even when dependencies keep changing.
            tokio::select! { _=tokio::signal::ctrl_c()=>break, _=terminate()=>break, task=tasks.join_next()=>{ bail!("background task stopped: {task:?}"); }, _=tokio::time::sleep(delay)=>() }
            tokio::select! { _=tokio::signal::ctrl_c()=>break, _=terminate()=>break, task=tasks.join_next()=>{ bail!("background task stopped: {task:?}"); }, _=signal.notified()=>(), _=tokio::time::sleep(Duration::from_secs(30))=>() }
        }
        tasks.abort_all();
        Ok(())
    }
    async fn terminate() {
        #[cfg(unix)]
        {
            if let Ok(mut s) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            {
                s.recv().await;
            }
        }
        #[cfg(not(unix))]
        std::future::pending::<()>().await;
    }
}
