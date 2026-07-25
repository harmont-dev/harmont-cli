//! Local secret resolution for `hm run`.
//!
//! A pipeline node carries a `secrets` map (env-var-name -> secret-name)
//! alongside its literal `env`. For a local run those references are
//! resolved to values from a project `.env` file, overlaid by the live
//! process environment (process env wins). A reference that cannot be
//! resolved is a hard, fail-fast error — we never inject an empty value.

use std::collections::BTreeMap;
use std::path::Path;

/// Resolves secret references for a local `hm run` from a project `.env` file
/// overlaid by the process environment (process env wins).
#[derive(Debug, Clone)]
pub(crate) struct SecretResolver {
    dotenv: BTreeMap<String, String>,
    proc_env: BTreeMap<String, String>,
}

/// A secret reference that could not be resolved from any source.
///
/// Carries both the referencing env var and the missing secret name so the
/// surfaced error can name precisely what failed and how to fix it.
#[derive(Debug, Clone)]
pub(crate) struct MissingSecret {
    pub env_var: String,
    pub secret_name: String,
}

impl SecretResolver {
    /// Construct a resolver from explicit `.env` and process-env maps.
    ///
    /// Test-only constructor; production code builds via
    /// [`SecretResolver::from_project_dir`].
    #[cfg(test)]
    pub(crate) const fn new(
        dotenv: BTreeMap<String, String>,
        proc_env: BTreeMap<String, String>,
    ) -> Self {
        Self { dotenv, proc_env }
    }

    /// Parse `<dir>/.env` if present; read the live process env.
    ///
    /// A missing or unreadable `.env` is not an error — secrets may all come
    /// from the process environment. Malformed lines in `.env` are skipped
    /// (via `flatten`) rather than aborting the run; a referenced secret that
    /// ends up unresolved still fails fast at [`Self::resolve`].
    pub(crate) fn from_project_dir(dir: &Path) -> Self {
        let mut dotenv = BTreeMap::new();
        let path = dir.join(".env");
        if path.exists()
            && let Ok(iter) = dotenvy::from_path_iter(&path)
        {
            for (k, v) in iter.flatten() {
                dotenv.insert(k, v);
            }
        }
        Self {
            dotenv,
            proc_env: std::env::vars().collect(),
        }
    }

    /// Look up a secret by name. Process env wins over `.env`.
    pub(crate) fn get(&self, name: &str) -> Option<String> {
        self.proc_env
            .get(name)
            .or_else(|| self.dotenv.get(name))
            .cloned()
    }

    /// Resolve env-var -> secret-name refs into env-var -> value. Fails fast on
    /// the first missing secret (`BTreeMap` iteration order is stable, so the
    /// reported missing secret is deterministic).
    ///
    /// # Errors
    /// Returns [`MissingSecret`] for the first reference whose secret name is
    /// not found in either the process env or the `.env` file.
    pub(crate) fn resolve(
        &self,
        refs: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>, MissingSecret> {
        let mut out = BTreeMap::new();
        for (env_var, secret_name) in refs {
            match self.get(secret_name) {
                Some(v) => {
                    out.insert(env_var.clone(), v);
                }
                None => {
                    return Err(MissingSecret {
                        env_var: env_var.clone(),
                        secret_name: secret_name.clone(),
                    });
                }
            }
        }
        Ok(out)
    }
}

/// Merge resolved secrets into a step's literal env map.
///
/// `env` is the step's already-merged literal environment; `secrets` is the
/// node's env-var -> secret-name reference map. On success the resolved
/// secret values are layered on top of `env` (a secret reference wins over a
/// same-named literal, matching the IR's "secrets resolved at run time"
/// contract) and the merged map is returned.
///
/// # Errors
/// Returns a [`MissingSecret`] when any referenced secret is unresolved, so
/// the caller can abort the step rather than silently inject an empty value.
pub(crate) fn resolve_step_env(
    mut env: BTreeMap<String, String>,
    secrets: &BTreeMap<String, String>,
    resolver: &SecretResolver,
) -> Result<BTreeMap<String, String>, MissingSecret> {
    let values = resolver.resolve(secrets)?;
    env.extend(values);
    Ok(env)
}
