#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::needless_raw_string_hashes
)]

#[tokio::test]
async fn python_roundtrip() {
    // Skip if python3 not available or harmont deps missing
    if which::which("python3").is_err() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".harmont");
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

    let lang = hm_dsl_engine::detect::detect_language(dir.path()).unwrap();
    assert_eq!(lang, hm_dsl_engine::DslLanguage::Python);

    let engine = hm_dsl_engine::engine_for(lang).unwrap();
    let metas = engine.list_pipelines(dir.path()).await.unwrap();
    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0].slug, "ci");

    let json_str = engine.render_pipeline_json(dir.path(), "ci").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(v["version"], "0");
}
