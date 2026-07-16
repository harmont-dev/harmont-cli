//! Secrets provider used when running harmont in the cloud.
use crate::domain::secrets;

/// Secret provider which injects secrets when running in the cloud.
///
/// Each step must announce which secrets it requires, and the cloud will only inject those secrets
/// into the executor. Note that no other process, except the executor, will be able to access the
/// secret, and you must **manually** inject the secret.
///
/// A naive approach would be to inject secrets directly into the environment, but that risks
/// leaking environments to processes which may not deserve those secrets, so we avoid doing that
/// here.
#[derive(Debug, Clone)]
pub(crate) struct CloudProvider;

impl secrets::Provider for CloudProvider {
    fn get(&self, _secret: &secrets::KeyRef<'_>) -> Option<&secrets::Value> {
        // TODO(markovejnovic): Implement
        None
    }

    fn list(&self) -> impl Iterator<Item = secrets::KeyRef<'_>> {
        // TODO(markovejnovic): Implement
        std::iter::empty()
    }
}
