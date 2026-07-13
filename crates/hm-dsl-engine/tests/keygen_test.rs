#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

//! Byte-exact parity checks against the Python `harmont.keygen` resolver.
//! Reference hashes are produced by running the Python resolver directly.

use std::collections::BTreeMap;
use std::path::PathBuf;

use hm_dsl_engine::keygen::{
    LowerOptions, compute_cache_key, env_subset, resolve_policy, sha256_hex,
};
use hm_dsl_engine::lower::lower_with_options;
use hm_dsl_engine::step_chain::{RawCachePolicy, RawStepChain};
use hm_pipeline_ir::PipelineGraph;

fn opts() -> LowerOptions {
    LowerOptions {
        pipeline_org: "myorg".into(),
        pipeline_slug: "ci".into(),
        now: 1_000_000,
        base_path: PathBuf::from("."),
        env: BTreeMap::from([
            ("RUST_VERSION".into(), "1.80".into()),
            ("OTHER".into(), "x".into()),
        ]),
    }
}

fn forever(env_keys: &[&str]) -> RawCachePolicy {
    RawCachePolicy::Forever {
        env_keys: env_keys.iter().map(ToString::to_string).collect(),
    }
}

fn cache_key_for(g: &PipelineGraph, step_key: &str) -> Option<String> {
    use daggy::petgraph::visit::IntoNodeReferences;
    g.dag()
        .graph()
        .node_references()
        .find(|(_, t)| t.step.key == step_key)
        .and_then(|(_, t)| t.step.cache.as_ref())
        .and_then(|c| c.key.clone())
}

#[test]
fn sha256_hex_matches_python() {
    assert_eq!(
        sha256_hex(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex("hello"),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn env_subset_sorts_and_appends_nul() {
    let env = BTreeMap::from([
        ("A".to_string(), "1".to_string()),
        ("B".to_string(), "2".to_string()),
        ("C".to_string(), "3".to_string()),
    ]);
    // Unsorted keys, missing "C" from the subset -> only A and B, sorted.
    assert_eq!(
        env_subset(&["B".to_string(), "A".to_string()], &env),
        "A=1\u{0}B=2\u{0}"
    );
}

#[test]
fn forever_policy_key_byte_exact() {
    let key = compute_cache_key("test", "cargo test", &forever(&["RUST_VERSION"]), "scratch", &opts())
        .unwrap();
    assert_eq!(
        key,
        "02cdd7a86196443ba51cac9e7e3987bb127af122f1386cfc6a843fe0c85e2499"
    );
}

#[test]
fn forever_policy_no_env_keys_byte_exact() {
    let key = compute_cache_key("test", "cargo test", &forever(&[]), "scratch", &opts()).unwrap();
    assert_eq!(
        key,
        "61b02abc60e2e2472835d4dbbc8f84c85197cbdda3af24c7cacf462ccaa410f8"
    );
}

#[test]
fn ttl_policy_key_byte_exact() {
    let policy = RawCachePolicy::Ttl {
        duration_seconds: 3600,
        env_keys: vec!["RUST_VERSION".into()],
    };
    // now = 1_000_000, bucket = 1_000_000 // 3600 = 277.
    let res = resolve_policy(&policy, "cargo test", &opts()).unwrap();
    assert_eq!(
        res,
        "ttl-277-6d7d30b69066a5c5913d44213c25029bc892aa872d95dd97511de7e2345f4ee8"
    );
    let key = compute_cache_key("test", "cargo test", &policy, "scratch", &opts()).unwrap();
    assert_eq!(
        key,
        "a937615f49e155edf8a059753c760bd285e0c088007f833f467864ada7cdbcab"
    );
}

#[test]
fn compose_policy_key_byte_exact() {
    let policy = RawCachePolicy::Compose {
        sub_policies: vec![
            forever(&["RUST_VERSION"]),
            RawCachePolicy::Ttl {
                duration_seconds: 3600,
                env_keys: vec!["RUST_VERSION".into()],
            },
        ],
    };
    let key = compute_cache_key("test", "cargo test", &policy, "scratch", &opts()).unwrap();
    assert_eq!(
        key,
        "d66b14459d672af0fe020875f8b411a0598afc23597a37e1dd0f47d5109bbda2"
    );
}

#[test]
fn compose_with_none_sub_byte_exact() {
    let policy = RawCachePolicy::Compose {
        sub_policies: vec![forever(&["RUST_VERSION"]), RawCachePolicy::None],
    };
    let key = compute_cache_key("test", "cargo test", &policy, "scratch", &opts()).unwrap();
    assert_eq!(
        key,
        "2b16eee0e344f933a360c7f8f1f88b0dd2692efd44690f949af8b27be4fe91a9"
    );
}

#[test]
fn parent_resolved_key_threads_through() {
    let parent = compute_cache_key("build", "cargo test", &forever(&["RUST_VERSION"]), "scratch", &opts())
        .unwrap();
    assert_eq!(
        parent,
        "0db96595eb9bcdadc0b34da262e816652127f6d19a7fe6f6b6b374c40629987a"
    );
    let child = compute_cache_key("test", "cargo build", &forever(&[]), &parent, &opts()).unwrap();
    assert_eq!(
        child,
        "3c9085b03deb8ce7034eabd47a83675793b92f69918c605b5a1f1a31575766a2"
    );
}

#[test]
fn on_change_file_dir_and_glob_byte_exact() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    std::fs::write(base.join("a.txt"), b"hello").unwrap();
    std::fs::create_dir(base.join("sub")).unwrap();
    std::fs::write(base.join("sub/b.txt"), b"world").unwrap();
    std::fs::write(base.join("sub/c.txt"), b"!").unwrap();

    let mut o = opts();
    o.base_path = base.to_path_buf();

    let on = |paths: &[&str]| {
        let policy = RawCachePolicy::OnChange {
            paths: paths.iter().map(ToString::to_string).collect(),
        };
        compute_cache_key("test", "cargo test", &policy, "scratch", &o).unwrap()
    };

    assert_eq!(
        on(&["a.txt"]),
        "1402470939ef19cd0e247f455f92f5430c9d3f1b6a7ebb59d2c1d249b39aecd4"
    );
    assert_eq!(
        on(&["sub"]),
        "556e5a08d960fb2799dc319ced1907d18bfec3458fd2396d0316682c84894408"
    );
    // Glob "*.txt" matches only a.txt at the top level -> same as ["a.txt"].
    assert_eq!(
        on(&["*.txt"]),
        "1402470939ef19cd0e247f455f92f5430c9d3f1b6a7ebb59d2c1d249b39aecd4"
    );
    // A plain missing path is silently skipped -> hashes the empty preimage.
    assert_eq!(
        on(&["nope.txt"]),
        "9dd1f573515149a2f42d1f7ed1ede5ca192d54c94f3367f6fcd4d3d3fc5b5c2c"
    );
    assert_eq!(
        on(&["a.txt", "sub"]),
        "93adc574b21838be2ee79ec97053a2d1b5c2f20dcd514162131c7eb83f2563db"
    );
}

#[test]
fn lower_with_options_resolves_chain_keys() {
    let chain: RawStepChain = serde_json::from_str(
        r#"{
            "steps": [
                {"cmd": "cargo build", "parent_idx": null, "label": "build",
                 "cache": {"policy": "forever"}},
                {"cmd": "cargo test", "parent_idx": 0, "label": "test",
                 "cache": {"policy": "forever", "env_keys": ["RUST_VERSION"]}}
            ],
            "leaf_indices": [1]
        }"#,
    )
    .unwrap();

    let g = lower_with_options(&chain, Some(&opts())).unwrap();
    assert_eq!(
        cache_key_for(&g, "build").as_deref(),
        Some("507e8361f96eac5eb350787dc29906644eb93456320ed7a19b877de9f3fa7456")
    );
    assert_eq!(
        cache_key_for(&g, "test").as_deref(),
        Some("15dee13d2b211a8bbe2647526a4a0735a42af9861dc4fbc4462d224dae8e53d5")
    );
}

#[test]
fn lower_without_options_leaves_keys_unresolved() {
    let chain: RawStepChain = serde_json::from_str(
        r#"{
            "steps": [
                {"cmd": "cargo test", "parent_idx": null, "label": "test",
                 "cache": {"policy": "forever"}}
            ],
            "leaf_indices": [0]
        }"#,
    )
    .unwrap();

    let g = lower_with_options(&chain, None).unwrap();
    assert_eq!(cache_key_for(&g, "test"), None);
}

#[test]
fn cached_step_with_uncached_parent_errors() {
    let chain: RawStepChain = serde_json::from_str(
        r#"{
            "steps": [
                {"cmd": "cargo build", "parent_idx": null, "label": "build"},
                {"cmd": "cargo test", "parent_idx": 0, "label": "test",
                 "cache": {"policy": "forever"}}
            ],
            "leaf_indices": [1]
        }"#,
    )
    .unwrap();

    let err = lower_with_options(&chain, Some(&opts())).unwrap_err();
    assert!(
        err.to_string().contains("no cached key"),
        "unexpected error: {err}"
    );
}
