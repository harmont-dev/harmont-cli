//! Convert a registry `LocalSpec` into a runnable `ServiceSpec`.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow};

use crate::orchestrator::docker_client::{ServiceSpec, ServiceSpecBuilder};

use super::naming::{
    DRIVER_LOCAL, LABEL_DRIVER, LABEL_SESSION, LABEL_SLUG, LABEL_WORKTREE, container_name,
};
use super::registry::{LocalSpec, PORT_SENTINEL};

/// Resolved spec for one deployment, ready to hand to
/// `DockerClient::start_service`. Borrows from the registry; held
/// alive by the up handler for the duration of the boot.
#[derive(Debug)]
pub struct ResolvedSpec {
    pub slug: String,
    pub container_name: String,
    pub image: String,
    pub env: Vec<String>,
    pub cmd: Option<Vec<String>>,
    pub workdir: Option<String>,
    pub binds: Vec<String>,
    pub publish: Vec<u16>,
    pub network: String,
    pub labels: HashMap<String, String>,
}

impl ResolvedSpec {
    #[must_use]
    pub fn as_service_spec(&self) -> ServiceSpec<'_> {
        ServiceSpecBuilder::new(&self.image, &self.container_name)
            .env(self.env.clone())
            .cmd(self.cmd.clone())
            .workdir(self.workdir.as_deref())
            .binds(self.binds.clone())
            .publish(self.publish.clone())
            .network(&self.network, &self.slug)
            .labels(self.labels.clone())
            .build()
    }
}

/// Build a `ResolvedSpec` from a `LocalSpec` + session metadata.
///
/// `image` is the resolved image tag (raw image from the spec, or the
/// committed tag for `from_=Step` builds — passed in by the up handler).
///
/// # Errors
///
/// Returns an error if any `port_mapping` key is not a valid `u16`, or
/// if a bind volume host path is not valid UTF-8.
pub fn build(
    slug: &str,
    spec: &LocalSpec,
    image: &str,
    worktree_root: &Path,
    worktree_hash: &str,
    session: &str,
    network: &str,
) -> Result<ResolvedSpec> {
    let env: Vec<String> = spec.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let binds = resolve_binds(worktree_root, &spec.volumes)?;
    let publish: Vec<u16> = spec
        .port_mapping
        .iter()
        .filter(|(_, sentinel)| sentinel.as_str() == PORT_SENTINEL)
        .map(|(cport, _)| {
            cport.parse::<u16>().context(format!(
                "port_mapping key `{cport}` is not a valid u16 — registry-dump bug?"
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut labels = HashMap::new();
    labels.insert(LABEL_WORKTREE.to_string(), worktree_hash.to_string());
    labels.insert(LABEL_SLUG.to_string(), slug.to_string());
    labels.insert(LABEL_SESSION.to_string(), session.to_string());
    labels.insert(LABEL_DRIVER.to_string(), DRIVER_LOCAL.to_string());
    Ok(ResolvedSpec {
        slug: slug.to_string(),
        container_name: container_name(worktree_hash, slug, session),
        image: image.to_string(),
        env,
        cmd: spec.cmd.clone(),
        workdir: spec.workdir.clone(),
        binds,
        publish,
        network: network.to_string(),
        labels,
    })
}

fn resolve_binds(
    worktree_root: &Path,
    volumes: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<String>> {
    let mut out = Vec::with_capacity(volumes.len());
    for (host, container) in volumes {
        let host_abs = if host.starts_with('/') {
            std::path::PathBuf::from(host)
        } else {
            worktree_root.join(host)
        };
        let host_str = host_abs
            .to_str()
            .ok_or_else(|| anyhow!("bind host path is not valid UTF-8: {}", host_abs.display()))?;
        // container may carry a `:ro` suffix; split + reconstruct so we
        // emit "host:container[:ro]".
        let (cpath, mode) = match container.rsplit_once(':') {
            Some((p, m)) if m == "ro" || m == "rw" => (p, m),
            _ => (container.as_str(), "rw"),
        };
        out.push(if mode == "rw" {
            format!("{host_str}:{cpath}")
        } else {
            format!("{host_str}:{cpath}:{mode}")
        });
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn local_spec() -> LocalSpec {
        let mut port_mapping = BTreeMap::new();
        port_mapping.insert("5432".to_string(), PORT_SENTINEL.to_string());
        LocalSpec {
            image: Some("postgres:16".into()),
            from: None,
            cmd: None,
            port_mapping,
            env: BTreeMap::from([("POSTGRES_PASSWORD".to_string(), "dev".to_string())]),
            volumes: BTreeMap::new(),
            workdir: None,
            deps: vec![],
        }
    }

    #[test]
    fn builds_a_resolved_spec() {
        let rs = build(
            "db",
            &local_spec(),
            "postgres:16",
            Path::new("/tmp/wt"),
            "a1b2c3d4e5",
            "7a2f91",
            "hm-a1b2c3d4e5-7a2f91",
        )
        .unwrap();
        assert_eq!(rs.container_name, "hm-a1b2c3d4e5-db-7a2f91");
        assert_eq!(rs.publish, vec![5432]);
        assert!(rs.env.contains(&"POSTGRES_PASSWORD=dev".to_string()));
        assert_eq!(rs.labels[LABEL_SLUG], "db");
    }

    #[test]
    fn resolves_relative_volume_against_worktree_root() {
        let mut spec = local_spec();
        spec.volumes
            .insert(".".to_string(), "/workspace".to_string());
        let rs = build("web", &spec, "node:20", Path::new("/tmp/wt"), "a", "b", "n").unwrap();
        assert_eq!(rs.binds, vec!["/tmp/wt/.:/workspace".to_string()]);
    }

    #[test]
    fn preserves_ro_suffix_on_container_path() {
        let mut spec = local_spec();
        spec.volumes
            .insert(".".to_string(), "/workspace:ro".to_string());
        let rs = build("web", &spec, "node:20", Path::new("/tmp/wt"), "a", "b", "n").unwrap();
        assert_eq!(rs.binds, vec!["/tmp/wt/.:/workspace:ro".to_string()]);
    }
}
