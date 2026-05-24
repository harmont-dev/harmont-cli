#[tokio::test]
#[cfg_attr(
    not(feature = "embedded-python"),
    ignore = "requires embedded-python + cached cpython.wasm"
)]
async fn python_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".harmont");
    std::fs::create_dir_all(&harmont).unwrap();
    std::fs::write(
        harmont.join("ci.py"),
        r#"import harmont as hm

@hm.pipeline('ci')
def ci() -> hm.Step:
    return hm.scratch().run('echo test', label='test')
"#,
    )
    .unwrap();

    let lang = hm_dsl_engine::detect::detect_language(dir.path()).unwrap();
    assert_eq!(lang, hm_dsl_engine::DslLanguage::Python);

    let engine = hm_dsl_engine::engine_for(lang).await.unwrap();
    let metas = engine.list_pipelines(dir.path()).await.unwrap();
    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0].slug, "ci");

    let json = engine.render_pipeline_json(dir.path(), "ci").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["version"], "0");
}
