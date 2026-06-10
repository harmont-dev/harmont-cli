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

    let engine = hm_dsl_engine::engine_for(hm_dsl_engine::DslLanguage::Python).unwrap();
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

#[tokio::test]
async fn python_renders_dynamic_target_with_runtime_environment() {
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

@hm.target(dynamic=True)
def choose_build() -> hm.Step:
    command = 'go test ./...' if hm.env('LANGUAGE') == 'go' else 'cargo test'
    return hm.sh(command, label='selected build')

@hm.pipeline('ci')
def ci() -> hm.Step:
    return choose_build()
"#,
    )
    .unwrap();

    let engine = hm_dsl_engine::engine_for(hm_dsl_engine::DslLanguage::Python).unwrap();
    let mut context = hm_dsl_engine::DynamicContext::default();
    context.env.insert("LANGUAGE".into(), "go".into());

    let json = engine
        .render_target_json(dir.path(), "choose_build", &context)
        .await
        .unwrap();
    let fragment: serde_json::Value = serde_json::from_str(&json).unwrap();
    let node = &fragment["graph"]["nodes"][0];

    assert_eq!(fragment["version"], "0");
    assert_eq!(node["step"]["eval"]["type"], "cmd");
    assert_eq!(node["step"]["eval"]["cmd"], "go test ./...");
    assert_eq!(node["env"]["LANGUAGE"], "go");
}

#[tokio::test]
async fn python_dynamic_target_reports_unknown_name() {
    if which::which("python3").is_err() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir_all(&harmont).unwrap();
    std::fs::write(harmont.join("ci.py"), "import harmont as hm\n").unwrap();

    let engine = hm_dsl_engine::engine_for(hm_dsl_engine::DslLanguage::Python).unwrap();
    let error = engine
        .render_target_json(
            dir.path(),
            "missing_target",
            &hm_dsl_engine::DynamicContext::default(),
        )
        .await
        .unwrap_err();

    assert!(format!("{error:#}").contains("dynamic target 'missing_target' not found"));
}
