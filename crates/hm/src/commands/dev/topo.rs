//! Boot-plan topo sort over the local-driver subset of the registry.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow};

use super::registry::{DevRegistry, RegEntry};

/// A topo-sorted list of boot levels. Each inner Vec contains slugs
/// that can boot in parallel after all earlier levels are running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootPlan {
    pub levels: Vec<Vec<String>>,
}

impl BootPlan {
    /// Flat iterator over every slug in boot order.
    pub fn slugs(&self) -> impl Iterator<Item = &str> {
        self.levels.iter().flatten().map(String::as_str)
    }
}

/// Compute the boot plan over local-driver deployments.
///
/// - `requested`: explicit slug subset; empty means "all local".
/// - `no_deps`: when true, only the requested slugs are included.
/// - Otherwise, transitive deps of the requested slugs are pulled in.
///
/// # Errors
///
/// Returns an error if a requested slug isn't registered, isn't a
/// local-driver entry, or if the dep graph contains a cycle.
pub fn plan(reg: &DevRegistry, requested: &[String], no_deps: bool) -> Result<BootPlan> {
    let mut deps: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (slug, entry) in &reg.deployments {
        if let RegEntry::Local(spec) = entry {
            deps.insert(
                slug.as_str(),
                spec.deps.iter().map(String::as_str).collect(),
            );
        }
    }
    for s in requested {
        if !deps.contains_key(s.as_str()) {
            let exists = reg.deployments.contains_key(s);
            return Err(anyhow!(
                "hm: slug `{s}` {}",
                if exists {
                    "is not a local-driver deployment (use the matching driver's `up`)"
                } else {
                    "is not registered in this worktree's .harmont/"
                }
            ));
        }
    }
    let selected: BTreeSet<String> = if requested.is_empty() {
        deps.keys().map(ToString::to_string).collect()
    } else if no_deps {
        requested.iter().cloned().collect()
    } else {
        let mut out: BTreeSet<String> = BTreeSet::new();
        let mut stack: Vec<String> = requested.to_vec();
        while let Some(s) = stack.pop() {
            if out.insert(s.clone()) {
                for d in deps.get(s.as_str()).cloned().unwrap_or_default() {
                    if deps.contains_key(d) {
                        stack.push(d.to_string());
                    }
                }
            }
        }
        out
    };
    // Kahn's algorithm restricted to `selected`.
    let mut indeg: BTreeMap<String, usize> = selected
        .iter()
        .map(|s| {
            let count = deps
                .get(s.as_str())
                .map_or(0, |ds| ds.iter().filter(|d| selected.contains(**d)).count());
            (s.clone(), count)
        })
        .collect();
    let mut levels: Vec<Vec<String>> = Vec::new();
    while !indeg.is_empty() {
        let ready: Vec<String> = indeg
            .iter()
            .filter(|&(_, &c)| c == 0)
            .map(|(s, _)| s.clone())
            .collect();
        if ready.is_empty() {
            let unresolved: Vec<String> = indeg.keys().cloned().collect();
            return Err(anyhow!(
                "hm: dep cycle among deployments: {}",
                unresolved.join(", ")
            ));
        }
        for s in &ready {
            indeg.remove(s);
        }
        for (slug, count) in &mut indeg {
            if let Some(ds) = deps.get(slug.as_str()) {
                let removed = ds
                    .iter()
                    .filter(|d| ready.iter().any(|r| *r == **d))
                    .count();
                *count = count.saturating_sub(removed);
            }
        }
        levels.push(ready);
    }
    Ok(BootPlan { levels })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::super::registry::{DevRegistry, LocalSpec, RegEntry};
    use super::*;
    use std::collections::BTreeMap;

    fn reg(specs: &[(&str, &[&str])]) -> DevRegistry {
        let mut deployments = BTreeMap::new();
        for (slug, deps) in specs {
            deployments.insert(
                (*slug).to_string(),
                RegEntry::Local(LocalSpec {
                    image: Some("img".into()),
                    from: None,
                    cmd: None,
                    port_mapping: BTreeMap::new(),
                    env: BTreeMap::new(),
                    volumes: BTreeMap::new(),
                    workdir: None,
                    deps: deps.iter().map(|d| (*d).to_string()).collect(),
                }),
            );
        }
        DevRegistry {
            schema_version: "0".into(),
            worktree: "/tmp/wt".into(),
            deployments,
        }
    }

    #[test]
    fn empty_request_brings_up_everything() {
        let r = reg(&[("db", &[]), ("api", &["db"]), ("web", &["api"])]);
        let plan = plan(&r, &[], false).unwrap();
        assert_eq!(
            plan.levels,
            vec![
                vec!["db".to_string()],
                vec!["api".to_string()],
                vec!["web".to_string()],
            ]
        );
    }

    #[test]
    fn explicit_slug_pulls_in_transitive_deps() {
        let r = reg(&[("db", &[]), ("api", &["db"]), ("web", &["api"])]);
        let plan = plan(&r, &["web".to_string()], false).unwrap();
        let slugs: Vec<&str> = plan.slugs().collect();
        assert_eq!(slugs, vec!["db", "api", "web"]);
    }

    #[test]
    fn no_deps_skips_transitive() {
        let r = reg(&[("db", &[]), ("api", &["db"]), ("web", &["api"])]);
        let plan = plan(&r, &["web".to_string()], true).unwrap();
        let slugs: Vec<&str> = plan.slugs().collect();
        assert_eq!(slugs, vec!["web"]);
    }

    #[test]
    fn unknown_slug_errors() {
        let r = reg(&[("db", &[])]);
        let err = plan(&r, &["redis".to_string()], false).unwrap_err();
        assert!(err.to_string().contains("not registered"));
    }

    #[test]
    fn cycle_errors() {
        let r = reg(&[("a", &["b"]), ("b", &["a"])]);
        let err = plan(&r, &[], false).unwrap_err();
        assert!(err.to_string().contains("dep cycle"));
    }

    #[test]
    fn parallel_siblings_share_a_level() {
        let r = reg(&[("db", &[]), ("cache", &[]), ("api", &["db", "cache"])]);
        let plan = plan(&r, &[], false).unwrap();
        assert_eq!(plan.levels.len(), 2);
        // First level should contain both leaf deps (order is BTreeMap iteration order).
        let level0: BTreeSet<&str> = plan.levels[0].iter().map(String::as_str).collect();
        assert!(level0.contains("db"));
        assert!(level0.contains("cache"));
        assert_eq!(plan.levels[1], vec!["api".to_string()]);
    }
}
