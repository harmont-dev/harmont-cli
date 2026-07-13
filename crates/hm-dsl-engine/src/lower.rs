//! Lowering pass: [`RawStepChain`] → canonical v0 [`PipelineGraph`].
//!
//! Ports the Python lowering (`harmont-py/harmont/_pipeline.py` and
//! `_keys.py`) into Rust. The chain is a forest of steps referenced by
//! index; this walks it back from each leaf, topo-sorts parent-before-child,
//! drops scratch/fork passthrough nodes and `wait` barriers, resolves each
//! command step's cross-reference key, and emits the petgraph-serde graph the
//! IR deserializes from.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use anyhow::{Context, Result};
use hm_pipeline_ir::PipelineGraph;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::step_chain::{RawCachePolicy, RawStepChain};

/// Across-the-board default image for imageless root steps. The SDK's
/// toolchains assume an apt-capable base, so `ubuntu:24.04` is the universal
/// default; child steps boot from their parent's snapshot and stay imageless.
const DEFAULT_IMAGE: &str = "ubuntu:24.04";

/// Lower a raw step chain into the canonical [`PipelineGraph`].
///
/// # Errors
///
/// Returns an error if the emitted graph fails to deserialize into a
/// [`PipelineGraph`] — e.g. a step declares a zero-second timeout, which the
/// IR rejects at the wire boundary.
pub fn lower(chain: &RawStepChain) -> Result<PipelineGraph> {
    let ordered = topo_collect(chain);
    let command_steps: Vec<usize> = ordered
        .iter()
        .copied()
        .filter(|&i| chain.steps[i].cmd.is_some() && !chain.steps[i].is_wait)
        .collect();
    let keys = resolve_keys(chain, &command_steps);

    // Dense node indices in emission order, keyed by raw step index.
    let idx_by_raw: HashMap<usize, usize> = command_steps
        .iter()
        .enumerate()
        .map(|(node_idx, &raw)| (raw, node_idx))
        .collect();

    let mut nodes: Vec<Value> = Vec::with_capacity(command_steps.len());
    let mut edges: Vec<Value> = Vec::new();

    // Command-step node indices emitted since the last `wait` barrier, and the
    // sources carried over from the most recent barrier.
    let mut pre_wait_indices: Vec<usize> = Vec::new();
    let mut pending_depends_on: Vec<usize> = Vec::new();

    for &raw in &ordered {
        let step = &chain.steps[raw];
        if step.is_wait {
            pending_depends_on = std::mem::take(&mut pre_wait_indices);
            continue;
        }
        let Some(cmd) = &step.cmd else {
            // scratch or fork — passthrough, not emitted as a node.
            continue;
        };

        let node_idx = idx_by_raw[&raw];
        let parent = resolved_parent_idx(chain, raw);

        let mut step_dict = Map::new();
        step_dict.insert("key".into(), Value::String(keys[&raw].clone()));
        step_dict.insert("cmd".into(), Value::String(cmd.clone()));
        if let Some(label) = &step.label {
            step_dict.insert("label".into(), Value::String(label.clone()));
        }
        if let Some(cache) = &step.cache {
            step_dict.insert("cache".into(), cache_to_value(cache));
        }
        if let Some(timeout) = step.timeout_seconds {
            step_dict.insert("timeout_seconds".into(), Value::from(timeout));
        }
        if let Some(image) = &step.image {
            step_dict.insert("image".into(), Value::String(image.clone()));
        }
        if let Some(runner) = &step.runner {
            step_dict.insert("runner".into(), Value::String(runner.clone()));
        }
        if let Some(runner_args) = &step.runner_args {
            step_dict.insert("runner_args".into(), runner_args.clone());
        }

        // Root command steps (no builds_in parent) that declare no image get
        // the pipeline-wide default; child steps inherit the parent snapshot.
        if parent.is_none() && !step_dict.contains_key("image") {
            step_dict.insert("image".into(), Value::String(DEFAULT_IMAGE.into()));
        }

        nodes.push(json!({"step": Value::Object(step_dict), "env": merged_env(chain, raw)}));

        if let Some(parent_raw) = parent {
            edges.push(json!([idx_by_raw[&parent_raw], node_idx, "builds_in"]));
        }
        for &dep in &pending_depends_on {
            edges.push(json!([dep, node_idx, "depends_on"]));
        }
        pre_wait_indices.push(node_idx);
    }

    let mut top = Map::new();
    top.insert("version".into(), Value::String("0".into()));
    if let Some(timeout) = chain.pipeline_timeout_seconds {
        top.insert("timeout_seconds".into(), Value::from(timeout));
    }
    top.insert(
        "graph".into(),
        json!({
            "nodes": nodes,
            "node_holes": [],
            "edge_property": "directed",
            "edges": edges,
        }),
    );

    serde_json::from_value(Value::Object(top))
        .context("failed to deserialize lowered pipeline graph")
}

/// Merge env in layers: non-interactive baseline, then pipeline-level, then
/// per-step overrides.
fn merged_env(chain: &RawStepChain, raw: usize) -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([
        ("DEBIAN_FRONTEND".to_string(), "noninteractive".to_string()),
        ("TERM".to_string(), "dumb".to_string()),
    ]);
    if let Some(pipeline_env) = &chain.pipeline_env {
        env.extend(pipeline_env.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    if let Some(step_env) = &chain.steps[raw].env {
        env.extend(step_env.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    env
}

/// Collect every step reachable from `leaves` via `parent_idx`, in
/// parent-before-child order. Tiebreak by leaf order, then DFS-pre on each leaf
/// chain. `wait` leaves are inserted at their leaf position.
fn topo_collect(chain: &RawStepChain) -> Vec<usize> {
    let mut seen: HashSet<usize> = HashSet::new();
    let mut ordered: Vec<usize> = Vec::new();

    for &leaf in &chain.leaf_indices {
        if chain.steps[leaf].is_wait {
            ordered.push(leaf);
            continue;
        }
        // Walk leaf -> root, stopping at the first already-seen ancestor.
        let mut walk: Vec<usize> = Vec::new();
        let mut node = Some(leaf);
        while let Some(n) = node {
            if seen.contains(&n) {
                break;
            }
            walk.push(n);
            node = chain.steps[n].parent_idx;
        }
        for &s in walk.iter().rev() {
            if seen.insert(s) {
                ordered.push(s);
            }
        }
    }
    ordered
}

/// Walk back through scratch/fork nodes to the nearest emitted command
/// ancestor, returning its raw index (the `builds_in` parent).
fn resolved_parent_idx(chain: &RawStepChain, raw: usize) -> Option<usize> {
    let mut node = chain.steps[raw].parent_idx;
    while let Some(n) = node {
        let step = &chain.steps[n];
        if step.cmd.is_some() && !step.is_wait {
            return Some(n);
        }
        node = step.parent_idx;
    }
    None
}

/// Render a raw cache policy to its IR `Cache` shape. Only the policy name
/// survives here; cache-key resolution is a separate pass.
fn cache_to_value(cache: &RawCachePolicy) -> Value {
    let policy = match cache {
        RawCachePolicy::None => "none",
        RawCachePolicy::Forever { .. } => "forever",
        RawCachePolicy::Ttl { .. } => "ttl",
        RawCachePolicy::OnChange { .. } => "on_change",
        RawCachePolicy::Compose { .. } => "compose",
    };
    json!({ "policy": policy })
}

/// Resolve each command step's cross-reference key, keyed by raw index.
///
/// Precedence: explicit `key_override`, then a unique slugified label, then a
/// stable hash of `(parent_key, cmd, position)`. When two steps' natural slugs
/// collide and neither claimed it via override, both fall back to hash; an
/// override reserves its string even against a peer's identical natural slug.
fn resolve_keys(chain: &RawStepChain, command_steps: &[usize]) -> HashMap<usize, String> {
    let mut overrides: HashMap<usize, String> = HashMap::new();
    let mut natural_slugs: HashMap<usize, String> = HashMap::new();
    for &raw in command_steps {
        let step = &chain.steps[raw];
        if let Some(over) = &step.key_override {
            overrides.insert(raw, over.clone());
        }
        if let Some(label) = &step.label {
            let slug = slugify_label(label);
            if !slug.is_empty() {
                natural_slugs.insert(raw, slug);
            }
        }
    }

    // Every override reserves its string; a natural slug matching a reserved
    // override collides for the slug claimant.
    let mut reserved: HashSet<String> = overrides.values().cloned().collect();

    // Slug collision counts span every labeled step, including override-bearing
    // ones — an override step still "claims" its natural slug.
    let mut slug_counts: HashMap<&str, usize> = HashMap::new();
    for slug in natural_slugs.values() {
        *slug_counts.entry(slug.as_str()).or_insert(0) += 1;
    }

    let mut keys: HashMap<usize, String> = HashMap::new();
    for (position, &raw) in command_steps.iter().enumerate() {
        if let Some(over) = overrides.get(&raw) {
            keys.insert(raw, over.clone());
            continue;
        }
        if let Some(slug) = natural_slugs.get(&raw)
            && !reserved.contains(slug)
            && slug_counts.get(slug.as_str()) == Some(&1)
        {
            reserved.insert(slug.clone());
            keys.insert(raw, slug.clone());
            continue;
        }
        // Hash fallback. The parent key is the direct parent's key only when
        // that parent is itself an already-resolved command step.
        let parent_key = chain.steps[raw]
            .parent_idx
            .and_then(|pidx| keys.get(&pidx))
            .map_or("", String::as_str);
        let cmd = chain.steps[raw].cmd.as_deref().unwrap_or_default();
        keys.insert(raw, hash_key(parent_key, cmd, position));
    }
    keys
}

/// Lowercase, strip `:emoji_codes:`, collapse non-alphanumeric runs to `-`,
/// and trim leading/trailing dashes. Non-ASCII characters are separators, so
/// slugs are ASCII-only; a label that reduces to empty falls back to a hash.
fn slugify_label(label: &str) -> String {
    let lowered = label.to_lowercase();
    let stripped = strip_emoji_shortcodes(&lowered);

    let mut out = String::with_capacity(stripped.len());
    let mut pending_dash = false;
    for c in stripped.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            pending_dash = false;
        } else if !pending_dash {
            pending_dash = true;
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Replace `:[a-z0-9_+-]+:` shortcodes with a space (matching Python's
/// `re.sub` on the already-lowercased label).
fn strip_emoji_shortcodes(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ':' {
            let mut j = i + 1;
            while j < chars.len() && is_shortcode_char(chars[j]) {
                j += 1;
            }
            // `+` needs at least one inner char, and a closing colon.
            if j > i + 1 && chars.get(j) == Some(&':') {
                out.push(' ');
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

const fn is_shortcode_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '+' | '-')
}

/// Stable 12-char SHA-256 prefix over `(parent_key, cmd, position)`.
fn hash_key(parent_key: &str, cmd: &str, position: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(parent_key.as_bytes());
    hasher.update([0u8]);
    hasher.update(cmd.as_bytes());
    hasher.update([0u8]);
    hasher.update(position.to_string().as_bytes());
    let digest = hasher.finalize();

    let mut out = String::with_capacity(12);
    for byte in digest.iter().take(6) {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
