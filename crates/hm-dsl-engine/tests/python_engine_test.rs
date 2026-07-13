#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::needless_raw_string_hashes
)]

use hm_dsl_engine::DslEngine;

#[tokio::test]
async fn python_roundtrip() {
    // Skip if python3 not available or harmont deps missing
    if which::which("python3").is_err() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir_all(&harmont).unwrap();
    std::fs::write(
        harmont.join("ci.py"),
        r#"import harmont as hm

@hm.pipeline('ci')
def ci() -> hm.Step:
    return hm.scratch().sh('echo test', label='test')
"#,
    )
    .unwrap();

    hm_dsl_engine::detect::check_python(dir.path()).unwrap();

    let engine = hm_dsl_engine::python_engine().unwrap();
    let metas = engine.list_pipelines(dir.path()).await.unwrap();
    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0].slug, "ci");

    let json_str = engine.render_pipeline_json(dir.path(), "ci").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(v["version"], "0");
}

#[tokio::test]
async fn python_registry_json_carries_triggers_and_allow_manual() {
    if which::which("python3").is_err() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir_all(&harmont).unwrap();
    std::fs::write(
        harmont.join("ci.py"),
        r#"import harmont as hm

@hm.pipeline('ci', name='CI', triggers=[hm.push(branch='main')], allow_manual=False)
def ci() -> hm.Step:
    return hm.scratch().sh('echo test', label='test')
"#,
    )
    .unwrap();

    let engine = hm_dsl_engine::python_engine().unwrap();
    let json = engine.registry_json(dir.path()).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();

    let p = &v["pipelines"][0];
    assert_eq!(p["slug"], "ci");
    assert_eq!(p["name"], "CI");
    assert_eq!(p["allow_manual"], false);
    assert_eq!(p["triggers"][0]["event"], "push");
    assert_eq!(p["triggers"][0]["branches"][0], "main");
    assert_eq!(p["definition"]["version"], "0");
}

/// A cached step's `cache.key` is resolved by the Rust lowering pass after the
/// Python subprocess emits the raw step chain — proving cache-key resolution
/// moved off the Python side.
#[tokio::test]
async fn python_render_resolves_cache_keys_in_rust() {
    if which::which("python3").is_err() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir_all(&harmont).unwrap();
    std::fs::write(
        harmont.join("ci.py"),
        r#"import harmont as hm

@hm.pipeline('ci')
def ci() -> hm.Step:
    return hm.scratch().sh('curl example.com | tar xz', label='fetch', cache=hm.forever())
"#,
    )
    .unwrap();

    let engine = hm_dsl_engine::python_engine().unwrap();
    let json_str = engine.render_pipeline_json(dir.path(), "ci").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    let cache = &v["graph"]["nodes"][0]["step"]["cache"];
    assert_eq!(cache["policy"], "forever");
    let key = cache["key"].as_str().expect("cache.key must be resolved");
    assert_eq!(key.len(), 64, "outer key is a sha256 hex digest: {key}");
    assert!(
        key.chars().all(|c| c.is_ascii_hexdigit()),
        "cache key is not hex: {key}"
    );
}
