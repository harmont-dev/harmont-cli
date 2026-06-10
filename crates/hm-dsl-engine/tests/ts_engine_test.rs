#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::needless_raw_string_hashes
)]

#[tokio::test]
async fn typescript_roundtrip() {
    // Skip if no JS runtime available
    if which::which("bun").is_err() && which::which("node").is_err() {
        eprintln!("skipping: no JS runtime on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir_all(&harmont).unwrap();
    std::fs::write(
        harmont.join("ci.ts"),
        r#"import { sh, pipeline, type PipelineDefinition } from '@harmont/hm';

const pipelines: PipelineDefinition[] = [
  {
    slug: 'ci',
    pipeline: pipeline([sh('echo test', { label: 'test' })])
  }
];

export default pipelines;
"#,
    )
    .unwrap();

    let lang = hm_dsl_engine::detect::detect_language(dir.path()).unwrap();
    assert_eq!(lang, hm_dsl_engine::DslLanguage::TypeScript);

    let engine = hm_dsl_engine::engine_for(lang).unwrap();
    let metas = engine.list_pipelines(dir.path()).await.unwrap();
    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0].slug, "ci");

    let json_str = engine.render_pipeline_json(dir.path(), "ci").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(v["version"], "0");
}

#[tokio::test]
async fn typescript_registry_json_carries_triggers_and_allow_manual() {
    // Skip if no JS runtime available
    if which::which("bun").is_err() && which::which("node").is_err() {
        eprintln!("skipping: no JS runtime on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir_all(&harmont).unwrap();
    std::fs::write(
        harmont.join("ci.ts"),
        r#"import { sh, pipeline, push, type PipelineDefinition } from '@harmont/hm';

const pipelines: PipelineDefinition[] = [
  {
    slug: 'ci',
    name: 'CI',
    allowManual: false,
    triggers: [push({ branch: 'main' })],
    pipeline: pipeline([sh('echo test', { label: 'test' })])
  }
];

export default pipelines;
"#,
    )
    .unwrap();

    let engine = hm_dsl_engine::engine_for(hm_dsl_engine::DslLanguage::TypeScript).unwrap();
    let json = engine.registry_json(dir.path()).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Envelope shape must be byte-for-byte parity with the Python
    // `dump_registry_json()` shape asserted in
    // python_engine_test::python_registry_json_carries_triggers_and_allow_manual.
    assert_eq!(v["schema_version"], "1");
    let p = &v["pipelines"][0];
    assert_eq!(p["slug"], "ci");
    assert_eq!(p["name"], "CI");
    assert_eq!(p["allow_manual"], false);
    assert_eq!(p["triggers"][0]["event"], "push");
    assert_eq!(p["triggers"][0]["branches"][0], "main");
    assert_eq!(p["definition"]["version"], "0");
}

#[tokio::test]
async fn typescript_named_export() {
    // Skip if no JS runtime available
    if which::which("bun").is_err() && which::which("node").is_err() {
        eprintln!("skipping: no JS runtime on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir_all(&harmont).unwrap();
    std::fs::write(
        harmont.join("ci.ts"),
        r#"import { sh, pipeline, type PipelineDefinition } from '@harmont/hm';

export const pipelines: PipelineDefinition[] = [
  {
    slug: 'ci',
    pipeline: pipeline([sh('echo test', { label: 'test' })])
  }
];
"#,
    )
    .unwrap();

    let engine = hm_dsl_engine::engine_for(hm_dsl_engine::DslLanguage::TypeScript).unwrap();
    let metas = engine.list_pipelines(dir.path()).await.unwrap();
    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0].slug, "ci");

    let json_str = engine.render_pipeline_json(dir.path(), "ci").await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(v["version"], "0");
}
