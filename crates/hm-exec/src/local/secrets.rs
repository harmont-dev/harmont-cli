//! Secrets provider used when running harmont locally.
use std::collections::BTreeMap;

use hm_util::path::AbsPathBuf;

use hm_core::Workspace;

use crate::domain::secrets;

/// Secret provider which fetches values from the local .env files.
///
/// A future impleemntation could support some sort of local store, to support, perhaps `hm secrets
/// store` and `hm secrets load`.
#[derive(Debug, Clone)]
pub(crate) struct LocalProvider {
    /// Stores the actual secrets locally.
    secret_store: BTreeMap<secrets::KeyBuf, secrets::Value>,
}

impl LocalProvider {
    /// Load the local provider given the path to the workspace.
    ///
    /// The local provider reads the workspace .env file and the `.hm/secrets` file. Values in the
    /// latter override the values in the former.
    pub(super) fn load(workspace: &Workspace) -> Self {
        // We first try to load secrets from the .env file, and then we patch those with the
        // .hm/secrets.toml file which takes priority.
        let mut secret_store = BTreeMap::new();

        let mut load_from_file = |path: AbsPathBuf| {
            if path.exists() && let Ok(iter) = dotenvy::from_path_iter(&path) {
                for (k, v) in iter.flatten() {
                    secret_store.insert(secrets::KeyBuf::new(k), secrets::Value::new(v));
                }
            }
        };

        load_from_file(workspace.env_file_path());
        load_from_file(workspace.secrets_path());

        Self { secret_store }
    }
}

impl secrets::Provider for LocalProvider {
    fn get(&self, secret: &secrets::KeyRef<'_>) -> Option<&secrets::Value> {
        self.secret_store.get(secret.as_str())
    }

    fn list(&self) -> impl Iterator<Item=secrets::KeyRef<'_>> {
        self.secret_store.keys().map(|k| secrets::KeyRef::new(k.as_ref()))
    }
}
