#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

#[tokio::test]
async fn cross_sdk_cache_keys_match() {
    // Skip if python3 not available
    if which::which("python3").is_err() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    // Skip if no JS runtime available
    if which::which("bun").is_err() && which::which("node").is_err() {
        eprintln!("skipping: no JS runtime on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir_all(&harmont).unwrap();

    // Write equivalent Python pipeline
    std::fs::write(
        harmont.join("ci.py"),
        r#"import harmont as hm

@hm.pipeline("ci")
def ci() -> tuple[hm.Step, ...]:
    base = hm.scratch().sh(
        "apt-get update && apt-get install -y gcc",
        label="setup",
        cache=hm.forever(),
    )
    build = base.sh("gcc -o main main.c", label="compile", cache=hm.forever())
    return (build,)
"#,
    )
    .unwrap();

    // Write equivalent TypeScript pipeline
    std::fs::write(
        harmont.join("ci.ts"),
        r#"import { sh, scratch, pipeline, forever, type PipelineDefinition } from '@harmont/hm';

const base = scratch().sh("apt-get update && apt-get install -y gcc", {
    label: "setup",
    cache: forever(),
});
const build = base.sh("gcc -o main main.c", { label: "compile", cache: forever() });

const pipelines: PipelineDefinition[] = [
    { slug: "ci", pipeline: pipeline([build]) },
];

export default pipelines;
"#,
    )
    .unwrap();

    // Run Python engine
    let py_engine = hm_dsl_engine::engine_for(hm_dsl_engine::DslLanguage::Python).unwrap();
    let py_json = py_engine
        .render_pipeline_json(dir.path(), "ci")
        .await
        .unwrap();
    let py_ir: serde_json::Value = serde_json::from_str(&py_json).unwrap();

    // Run TypeScript engine
    let ts_engine = hm_dsl_engine::engine_for(hm_dsl_engine::DslLanguage::TypeScript).unwrap();
    let ts_json = ts_engine
        .render_pipeline_json(dir.path(), "ci")
        .await
        .unwrap();
    let ts_ir: serde_json::Value = serde_json::from_str(&ts_json).unwrap();

    eprintln!(
        "Python IR:\n{}",
        serde_json::to_string_pretty(&py_ir).unwrap()
    );
    eprintln!(
        "TypeScript IR:\n{}",
        serde_json::to_string_pretty(&ts_ir).unwrap()
    );

    // Extract nodes from both IRs
    let py_nodes = py_ir["graph"]["nodes"]
        .as_array()
        .expect("Python IR should have graph.nodes array");
    let ts_nodes = ts_ir["graph"]["nodes"]
        .as_array()
        .expect("TypeScript IR should have graph.nodes array");

    // Assert same number of nodes
    assert_eq!(
        py_nodes.len(),
        ts_nodes.len(),
        "Node count mismatch: Python has {} nodes, TypeScript has {} nodes",
        py_nodes.len(),
        ts_nodes.len(),
    );

    // For each node pair: assert step.key and step.cache.key match
    for (i, (py_node, ts_node)) in py_nodes.iter().zip(ts_nodes.iter()).enumerate() {
        let py_step = &py_node["step"];
        let ts_step = &ts_node["step"];

        let py_key = &py_step["key"];
        let ts_key = &ts_step["key"];
        assert_eq!(
            py_key, ts_key,
            "Node {i}: step.key mismatch — Python={py_key}, TypeScript={ts_key}",
        );

        let py_cache_key = &py_step["cache"]["key"];
        let ts_cache_key = &ts_step["cache"]["key"];
        assert_eq!(
            py_cache_key, ts_cache_key,
            "Node {i} (step.key={py_key}): cache.key mismatch — Python={py_cache_key}, TypeScript={ts_cache_key}",
        );
    }

    // Assert edge structure matches
    let py_edges = py_ir["graph"]["edges"]
        .as_array()
        .expect("Python IR should have graph.edges array");
    let ts_edges = ts_ir["graph"]["edges"]
        .as_array()
        .expect("TypeScript IR should have graph.edges array");

    assert_eq!(
        py_edges.len(),
        ts_edges.len(),
        "Edge count mismatch: Python has {} edges, TypeScript has {} edges",
        py_edges.len(),
        ts_edges.len(),
    );

    for (i, (py_edge, ts_edge)) in py_edges.iter().zip(ts_edges.iter()).enumerate() {
        assert_eq!(
            py_edge, ts_edge,
            "Edge {i} mismatch — Python={py_edge}, TypeScript={ts_edge}",
        );
    }

    eprintln!("All cache keys match across Python and TypeScript SDKs!");
}

/// The JS toolchain pins the package-manager version from `packageManager`
/// (corepack install -g pnpm@<v>) and switches the bootstrap step's cache to
/// watch package.json. This must stay identical across the Python and
/// TypeScript SDKs, or pinned JS pipelines would produce divergent IR and
/// cache keys. Exercises the corepack-pin parity fix end to end.
#[tokio::test]
async fn cross_sdk_cache_keys_match_js_corepack_pin() {
    if which::which("python3").is_err() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    if which::which("bun").is_err() && which::which("node").is_err() {
        eprintln!("skipping: no JS runtime on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();

    // A `js` project directory with a package.json that pins the pnpm version
    // drives auto-detection in both SDKs. We bake the absolute path into the
    // DSL sources: the Python engine runs with cwd=project_dir but the TS
    // engine does not, so a bare `path="."` would diverge — an explicit
    // absolute path is the only thing both SDKs resolve identically.
    let proj_dir = dir.path().join("app");
    std::fs::create_dir_all(&proj_dir).unwrap();
    std::fs::write(
        proj_dir.join("package.json"),
        r#"{"packageManager": "pnpm@10.33.0"}"#,
    )
    .unwrap();
    std::fs::write(proj_dir.join("pnpm-lock.yaml"), "").unwrap();
    let proj_path = proj_dir.to_str().unwrap();

    let harmont = dir.path().join(".hm");
    std::fs::create_dir_all(&harmont).unwrap();

    std::fs::write(
        harmont.join("ci.py"),
        format!(
            r#"import harmont as hm

@hm.pipeline("ci")
def ci() -> tuple[hm.Step, ...]:
    proj = hm.js.project(path={proj_path:?})
    return (proj.run("test"),)
"#
        ),
    )
    .unwrap();

    std::fs::write(
        harmont.join("ci.ts"),
        format!(
            r#"import {{ pipeline, type PipelineDefinition }} from '@harmont/hm';
import {{ js }} from '@harmont/hm/toolchains';

const proj = js.project({{ path: {proj_path:?} }});

const pipelines: PipelineDefinition[] = [
    {{ slug: "ci", pipeline: pipeline([proj.run("test")]) }},
];

export default pipelines;
"#
        ),
    )
    .unwrap();

    let py_engine = hm_dsl_engine::engine_for(hm_dsl_engine::DslLanguage::Python).unwrap();
    let py_json = py_engine
        .render_pipeline_json(dir.path(), "ci")
        .await
        .unwrap();
    let py_ir: serde_json::Value = serde_json::from_str(&py_json).unwrap();

    let ts_engine = hm_dsl_engine::engine_for(hm_dsl_engine::DslLanguage::TypeScript).unwrap();
    let ts_json = ts_engine
        .render_pipeline_json(dir.path(), "ci")
        .await
        .unwrap();
    let ts_ir: serde_json::Value = serde_json::from_str(&ts_json).unwrap();

    eprintln!(
        "Python IR:\n{}",
        serde_json::to_string_pretty(&py_ir).unwrap()
    );
    eprintln!(
        "TypeScript IR:\n{}",
        serde_json::to_string_pretty(&ts_ir).unwrap()
    );

    let py_nodes = py_ir["graph"]["nodes"]
        .as_array()
        .expect("Python IR should have graph.nodes array");
    let ts_nodes = ts_ir["graph"]["nodes"]
        .as_array()
        .expect("TypeScript IR should have graph.nodes array");

    assert_eq!(
        py_nodes.len(),
        ts_nodes.len(),
        "Node count mismatch: Python has {} nodes, TypeScript has {} nodes",
        py_nodes.len(),
        ts_nodes.len(),
    );

    // The corepack bootstrap node must carry the pinned command in both SDKs.
    let py_cmds: Vec<&str> = py_nodes
        .iter()
        .filter_map(|n| n["step"]["cmd"].as_str())
        .collect();
    assert!(
        py_cmds.contains(&"corepack enable pnpm && corepack install -g pnpm@10.33.0"),
        "Python IR is missing the pinned corepack command; got {py_cmds:?}",
    );

    for (i, (py_node, ts_node)) in py_nodes.iter().zip(ts_nodes.iter()).enumerate() {
        let py_step = &py_node["step"];
        let ts_step = &ts_node["step"];

        let py_key = &py_step["key"];
        let ts_key = &ts_step["key"];
        assert_eq!(
            py_key, ts_key,
            "Node {i}: step.key mismatch — Python={py_key}, TypeScript={ts_key}",
        );

        let py_cache_key = &py_step["cache"]["key"];
        let ts_cache_key = &ts_step["cache"]["key"];
        assert_eq!(
            py_cache_key, ts_cache_key,
            "Node {i} (step.key={py_key}): cache.key mismatch — Python={py_cache_key}, TypeScript={ts_cache_key}",
        );
    }

    let py_edges = py_ir["graph"]["edges"]
        .as_array()
        .expect("Python IR should have graph.edges array");
    let ts_edges = ts_ir["graph"]["edges"]
        .as_array()
        .expect("TypeScript IR should have graph.edges array");

    assert_eq!(
        py_edges.len(),
        ts_edges.len(),
        "Edge count mismatch: Python has {} edges, TypeScript has {} edges",
        py_edges.len(),
        ts_edges.len(),
    );

    for (i, (py_edge, ts_edge)) in py_edges.iter().zip(ts_edges.iter()).enumerate() {
        assert_eq!(
            py_edge, ts_edge,
            "Edge {i} mismatch — Python={py_edge}, TypeScript={ts_edge}",
        );
    }

    eprintln!("JS corepack-pin cache keys match across Python and TypeScript SDKs!");
}
