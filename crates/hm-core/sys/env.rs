use std::sync::OnceLock;

use secrecy::SecretString;

static HM_API_TOKEN: OnceLock<Option<SecretString>> = OnceLock::new();

/// The `HM_API_TOKEN` override, if set to a non-empty value.
///
/// An empty `HM_API_TOKEN` is treated as unset: CI commonly exports an unset
/// secret as the empty string, and honoring it would send an empty bearer
/// (a 401) instead of falling back to the stored credential.
///
/// Read once per process — `hm` is a CLI, so the environment does not change
/// under us.
pub fn hm_api_token() -> &'static Option<SecretString> {
    HM_API_TOKEN.get_or_init(|| {
        std::env::var("HM_API_TOKEN")
            .ok()
            .filter(|t| !t.is_empty())
            .map(SecretString::from)
    })
}
