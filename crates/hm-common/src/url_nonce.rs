//! Cryptographic nonces.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore as _;
use subtle::ConstantTimeEq as _;
use zeroize::ZeroizeOnDrop;

/// A nonce carrying 256 bits of cryptographic entropy.
#[derive(Clone, ZeroizeOnDrop)]
pub struct UrlNonce([u8; 32]);

impl UrlNonce {
    /// Generate a fresh nonce from the operating system's CSPRNG.
    #[must_use]
    pub fn random() -> Self {
        let mut bytes = [0u8; 32];
        let mut rng = rand::rngs::OsRng;
        rng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// The nonce as a URL-safe base64 string.
    #[must_use]
    pub fn base_64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// Whether `candidate` equals this nonce's base64 form, compared in
    /// constant time.
    #[must_use]
    pub fn verify(&self, candidate: &str) -> bool {
        self.base_64().as_bytes().ct_eq(candidate.as_bytes()).into()
    }
}

impl std::fmt::Debug for UrlNonce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UrlNonce(<redacted>)")
    }
}

impl PartialEq for UrlNonce {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_slice().ct_eq(other.0.as_slice()).into()
    }
}

impl Eq for UrlNonce {}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test setup and assertions")]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    fn random_nonces_are_distinct() {
        assert_ne!(UrlNonce::random(), UrlNonce::random());
    }

    #[rstest]
    fn base_64_decodes_to_32_bytes() {
        let nonce = UrlNonce::random();
        let raw = URL_SAFE_NO_PAD.decode(nonce.base_64()).unwrap();
        assert_eq!(raw.len(), 32);
    }

    #[rstest]
    fn equals_its_clone_and_verifies_in_place() {
        let nonce = UrlNonce::random();
        assert_eq!(nonce.clone(), nonce);
        assert!(nonce.verify(&nonce.base_64()));
        assert!(!nonce.verify("not-the-nonce"));
    }

    #[rstest]
    fn debug_does_not_leak_the_value() {
        let nonce = UrlNonce::random();
        let rendered = format!("{nonce:?}");
        assert!(!rendered.contains(&nonce.base_64()));
    }
}
