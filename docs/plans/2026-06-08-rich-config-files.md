# Rich Configuration Files Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add hierarchical TOML config with user-level (`~/.config/hm/config.toml`) and project-level (`<project>/.harmont/config.toml`) override semantics, anchored by a `[cloud]` section carrying `org`.

**Architecture:** Figment-based layered config. Single `Config` struct with serde defaults — figment merges sources (defaults → user file → project file → env vars) and extracts the final struct. No manual merge logic. Project root discovered by walking up from cwd looking for `.harmont/`. Breaks existing `~/.harmont/config.toml` format — no legacy fallback.

**Tech Stack:** `figment` 0.10 (new dep, features: `toml`, `env`), `serde` + `toml` 0.8 (already in workspace), `dirs` 6 (via `hm-util`).

**Config schema:**

```toml
# ~/.config/hm/config.toml  OR  <project>/.harmont/config.toml

[cloud]
org = "my-org"                           # organization slug
api_url = "https://api.harmont.dev"      # cloud API base (optional)

[preferences]
format = "human"                         # "human" or "json"
auto_watch = false                       # auto-watch on build create
```

**Env var override:** Figment `Env::prefixed("HM_").split("__")` — e.g. `HM_CLOUD__ORG=foo` maps to `cloud.org`.

**Crate placement:** Config types and figment loading in `crates/hm/src/config.rs`. Dir accessor in `hm-util/src/dirs.rs`. Project root discovery in `hm-util/src/dirs.rs`.

---

### Task 1: Add figment to workspace dependencies

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/hm/Cargo.toml`

**Step 1: Add figment to workspace deps**

In workspace root `Cargo.toml`, add to `[workspace.dependencies]`:

```toml
figment = { version = "0.10", features = ["toml", "env"] }
```

**Step 2: Add figment to hm crate deps**

In `crates/hm/Cargo.toml`, add to `[dependencies]`:

```toml
figment = { workspace = true }
```

**Step 3: Verify it builds**

Run: `cargo check -p harmont-cli`
Expected: PASS

**Step 4: Commit**

```bash
git add Cargo.toml crates/hm/Cargo.toml
git commit -m "deps: add figment 0.10 for hierarchical config"
```

---

### Task 2: Add XDG user config dir accessor to hm-util

**Files:**
- Modify: `crates/hm-util/src/dirs.rs`

**Step 1: Write the failing test**

Add to the `tests` module in `crates/hm-util/src/dirs.rs`:

```rust
#[test]
fn hm_user_config_dir_under_config() {
    let p = hm_user_config_dir().unwrap();
    assert!(p.ends_with("hm"), "expected path ending in 'hm', got {p:?}");
    let parent = p.parent().unwrap();
    assert!(
        parent.ends_with(".config") || parent.ends_with("AppData/Roaming"),
        "unexpected parent: {parent:?}"
    );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p hm-util hm_user_config_dir_under_config`
Expected: FAIL — `hm_user_config_dir` does not exist.

**Step 3: Write the implementation**

In `crates/hm-util/src/dirs.rs`, add:

```rust
/// `<config_dir>/hm/` — XDG-aware user config root for `config.toml`.
///
/// - Linux/macOS: `~/.config/hm/`
/// - Windows: `{FOLDERID_RoamingAppData}/hm/`
pub fn hm_user_config_dir() -> Option<PathBuf> {
    platform::config_dir().map(|c| c.join("hm"))
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p hm-util hm_user_config_dir_under_config`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/hm-util/src/dirs.rs
git commit -m "feat(hm-util): add hm_user_config_dir for XDG config path"
```

---

### Task 3: Add project root discovery to hm-util

**Files:**
- Modify: `crates/hm-util/src/dirs.rs`

Walk up from a starting path looking for `.harmont/` directory (like git's `.git/` discovery).

**Step 1: Write the failing tests**

```rust
#[test]
fn find_project_root_at_current_dir() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join(".harmont")).unwrap();
    let found = find_project_root(tmp.path());
    assert_eq!(found, Some(tmp.path().to_path_buf()));
}

#[test]
fn find_project_root_walks_up() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join(".harmont")).unwrap();
    let nested = tmp.path().join("src").join("deep");
    std::fs::create_dir_all(&nested).unwrap();
    let found = find_project_root(&nested);
    assert_eq!(found, Some(tmp.path().to_path_buf()));
}

#[test]
fn find_project_root_returns_none_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let found = find_project_root(tmp.path());
    assert_eq!(found, None);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p hm-util find_project_root`
Expected: FAIL — `find_project_root` does not exist.

**Step 3: Write the implementation**

```rust
/// Walk up from `start` looking for a directory containing `.harmont/`.
/// Returns the project root (the directory *containing* `.harmont/`),
/// or `None` if the filesystem root is reached without finding one.
pub fn find_project_root(start: &std::path::Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        if current.join(".harmont").is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p hm-util find_project_root`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/hm-util/src/dirs.rs
git commit -m "feat(hm-util): add find_project_root — walk-up discovery for .harmont/"
```

---

### Task 4: Define Config types with figment-based loader

**Files:**
- Modify: `crates/hm/src/config.rs`

This replaces the entire existing `config.rs`. The old `Config`, `Preferences`, `user_config_dir()`, `Config::load()`, `Config::save()`, `Config::path()` are all deleted.

**Step 1: Write the failing tests**

Replace any existing tests in `config.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = Config::default();
        assert!(config.cloud.org.is_none());
        assert_eq!(config.cloud.api_url, DEFAULT_API_URL);
        assert_eq!(config.preferences.format, "human");
        assert!(!config.preferences.auto_watch);
    }

    #[test]
    fn deserialize_full_toml() {
        let toml_str = r#"
[cloud]
org = "test-org"
api_url = "https://test.api"

[preferences]
format = "json"
auto_watch = true
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.cloud.org.as_deref(), Some("test-org"));
        assert_eq!(config.cloud.api_url, "https://test.api");
        assert_eq!(config.preferences.format, "json");
        assert!(config.preferences.auto_watch);
    }

    #[test]
    fn deserialize_sparse_toml() {
        let toml_str = "[cloud]\norg = \"sparse-org\"\n";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.cloud.org.as_deref(), Some("sparse-org"));
        assert_eq!(config.cloud.api_url, DEFAULT_API_URL);
        assert_eq!(config.preferences.format, "human");
    }

    #[test]
    fn deserialize_empty_toml() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.cloud.org.is_none());
        assert_eq!(config.cloud.api_url, DEFAULT_API_URL);
    }

    #[test]
    fn figment_project_overrides_user() {
        let user_dir = tempfile::tempdir().unwrap();
        let user_path = user_dir.path().join("config.toml");
        std::fs::write(
            &user_path,
            b"[cloud]\norg = \"user-org\"\napi_url = \"https://user.api\"\n",
        )
        .unwrap();

        let project_dir = tempfile::tempdir().unwrap();
        let project_path = project_dir.path().join("config.toml");
        std::fs::write(
            &project_path,
            b"[cloud]\norg = \"project-org\"\n",
        )
        .unwrap();

        let config = Config::load_from_paths(Some(&user_path), Some(&project_path))?;
        assert_eq!(config.cloud.org.as_deref(), Some("project-org"));
        // api_url not overridden by project — keeps user value
        assert_eq!(config.cloud.api_url, "https://user.api");
    }

    #[test]
    fn figment_missing_files_still_resolve() {
        let config = Config::load_from_paths(
            Some(Path::new("/nonexistent/user.toml")),
            Some(Path::new("/nonexistent/project.toml")),
        )?;
        assert_eq!(config.cloud.api_url, DEFAULT_API_URL);
        assert_eq!(config.preferences.format, "human");
    }
}
```

Note: tests using `?` should return `Result<(), figment::Error>` or `Result<(), Box<dyn std::error::Error>>`.

**Step 2: Run tests to verify they fail**

Run: `cargo test -p harmont-cli --lib config::tests`
Expected: FAIL — new types and `load_from_paths` do not exist.

**Step 3: Write the implementation**

Replace the entire `config.rs`:

```rust
use anyhow::{Context, Result};
use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_API_URL: &str = "https://api.harmont.dev";

fn default_api_url() -> String {
    DEFAULT_API_URL.into()
}

fn default_format() -> String {
    "human".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloudConfig {
    pub org: Option<String>,
    #[serde(default = "default_api_url")]
    pub api_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub auto_watch: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            format: default_format(),
            auto_watch: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub cloud: CloudConfig,
    #[serde(default)]
    pub preferences: Preferences,
}

impl Config {
    /// User-level config path: `~/.config/hm/config.toml`.
    pub fn user_config_path() -> Result<PathBuf> {
        hm_util::dirs::hm_user_config_dir()
            .map(|d| d.join("config.toml"))
            .context("could not determine user config directory")
    }

    /// Project-level config path: `<root>/.harmont/config.toml`.
    pub fn project_config_path(project_root: &Path) -> PathBuf {
        project_root.join(".harmont").join("config.toml")
    }

    /// Load config with full layering: defaults → user → project → env.
    pub fn load(project_root: Option<&Path>) -> Result<Self> {
        let user_path = Self::user_config_path().ok();
        let project_path = project_root.map(Self::project_config_path);
        Self::load_from_paths(
            user_path.as_deref(),
            project_path.as_deref(),
        )
        .map_err(|e| anyhow::anyhow!("config error: {e}"))
    }

    /// Core loader taking explicit paths (testable without real HOME).
    pub fn load_from_paths(
        user_path: Option<&Path>,
        project_path: Option<&Path>,
    ) -> std::result::Result<Self, figment::Error> {
        let mut figment = Figment::new()
            .merge(Serialized::defaults(Self::default()));

        if let Some(p) = user_path {
            figment = figment.merge(Toml::file(p));
        }
        if let Some(p) = project_path {
            figment = figment.merge(Toml::file(p));
        }

        figment
            .merge(Env::prefixed("HM_").split("__"))
            .extract()
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p harmont-cli --lib config::tests`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/hm/src/config.rs
git commit -m "feat(config): figment-based Config with layered TOML + env loading"
```

---

### Task 5: Wire Config into RunContext and CLI

**Files:**
- Modify: `crates/hm/src/context.rs`
- Potentially modify: any file in `crates/hm/src/` that references the old `Config` type

The old `Config` had `config.api_url()` (method) and `config.org` (flat field). The new `Config` has `config.cloud.api_url` and `config.cloud.org`.

**Step 1: Grep for all old Config usages**

Run: `grep -rn 'config\.\(api_url\|org\b\|preferences\)' crates/hm/src/`
Run: `grep -rn 'Config::' crates/hm/src/`
Run: `grep -rn 'use crate::config::' crates/hm/src/`

Understand every call site before changing anything.

**Step 2: Update `context.rs`**

```rust
use std::io::IsTerminal;

use anyhow::Result;

use crate::cli::Cli;
use crate::config::Config;
use crate::output::OutputMode;

#[derive(Debug)]
pub struct RunContext {
    pub config: Config,
    pub output: OutputMode,
}

impl RunContext {
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        let start_dir = std::env::current_dir()
            .context("cannot determine current directory")?;
        let project_root = hm_util::dirs::find_project_root(&start_dir);
        let config = Config::load(project_root.as_deref())?;

        let color =
            !cli.no_color && std::env::var("NO_COLOR").is_err() && std::io::stderr().is_terminal();

        let output = OutputMode::Human {
            color,
            interactive: std::io::stdout().is_terminal(),
        };

        Ok(Self { config, output })
    }
}
```

**Step 3: Update all call sites**

Transform each usage:
- `ctx.config.api_url()` → `ctx.config.cloud.api_url` (it's `String` now, use `&ctx.config.cloud.api_url` or `.as_str()` as needed)
- `ctx.config.org` → `ctx.config.cloud.org`
- `ctx.config.preferences.format` → `ctx.config.preferences.format` (unchanged path, but old field was `Option<String>`, now `String`)
- `ctx.config.preferences.auto_watch` → `ctx.config.preferences.auto_watch` (was `Option<bool>`, now `bool`)

Remove the old `user_config_dir()` function if it's still being imported elsewhere — it's been superseded by `Config::user_config_path()`.

**Step 4: Delete old `Config::save()` and `Config::path()` methods**

These don't exist in the new implementation. If any code calls them, it needs updating. Check `creds_store.rs` — it may reference `user_config_dir()` for the credentials path. That function was in `config.rs` but credentials should use `hm_util::dirs::harmont_config_dir()` directly.

**Step 5: Build and fix compilation errors**

Run: `cargo build -p harmont-cli`
Expected: PASS after fixing all call sites.

**Step 6: Run all existing tests**

Run: `cargo test -p harmont-cli`
Expected: PASS (some tests may need updating if they construct the old `Config` directly).

**Step 7: Commit**

```bash
git add crates/hm/src/
git commit -m "feat(config): wire figment Config into RunContext with project root discovery"
```

---

### Task 6: Add save support

**Files:**
- Modify: `crates/hm/src/config.rs`

Save writes a `Config` to a TOML file atomically. Needed for future `hm cloud org switch` write-back.

**Step 1: Write the failing test**

```rust
#[test]
fn save_and_reload_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    let config = Config {
        cloud: CloudConfig {
            org: Some("saved-org".into()),
            api_url: DEFAULT_API_URL.into(),
        },
        preferences: Preferences::default(),
    };
    config.save_to(&path).unwrap();

    let loaded: Config = toml::from_str(
        &std::fs::read_to_string(&path).unwrap()
    ).unwrap();
    assert_eq!(loaded.cloud.org.as_deref(), Some("saved-org"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p harmont-cli --lib config::tests::save`
Expected: FAIL — `save_to` does not exist.

**Step 3: Write the implementation**

```rust
impl Config {
    /// Persist config to `path` atomically.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        let serialized = toml::to_string_pretty(self).context("serializing config")?;
        hm_util::os::fs::blocking::write_atomic_restricted(
            path,
            serialized.as_bytes(),
            0o644,
            0o700,
        )
        .with_context(|| format!("writing {}", path.display()))
    }

    /// Save to user-level config path (`~/.config/hm/config.toml`).
    pub fn save_user(&self) -> Result<()> {
        self.save_to(&Self::user_config_path()?)
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p harmont-cli --lib config::tests::save`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/hm/src/config.rs
git commit -m "feat(config): add save_to / save_user for atomic config writes"
```

---

### Task 7: Integration tests — layered config end-to-end

**Files:**
- Create: `crates/hm/tests/config_layered.rs`

**Step 1: Write the integration tests**

```rust
//! End-to-end test: project config overrides user config via figment.

use std::fs;
use tempfile::tempdir;

#[test]
fn project_overrides_user() {
    let user_dir = tempdir().unwrap();
    let user_path = user_dir.path().join("config.toml");
    fs::write(
        &user_path,
        b"[cloud]\norg = \"user-org\"\napi_url = \"https://user.api\"\n\n[preferences]\nformat = \"json\"\n",
    )
    .unwrap();

    let project_dir = tempdir().unwrap();
    let project_path = project_dir.path().join("config.toml");
    fs::write(&project_path, b"[cloud]\norg = \"project-org\"\n").unwrap();

    let config = harmont_cli::config::Config::load_from_paths(
        Some(&user_path),
        Some(&project_path),
    )
    .unwrap();

    assert_eq!(config.cloud.org.as_deref(), Some("project-org"));
    assert_eq!(config.cloud.api_url, "https://user.api");
    assert_eq!(config.preferences.format, "json");
}

#[test]
fn missing_files_resolve_to_defaults() {
    let config = harmont_cli::config::Config::load_from_paths(None, None).unwrap();
    assert_eq!(config.cloud.api_url, "https://api.harmont.dev");
    assert_eq!(config.preferences.format, "human");
    assert!(!config.preferences.auto_watch);
    assert!(config.cloud.org.is_none());
}

#[test]
fn project_only_no_user() {
    let project_dir = tempdir().unwrap();
    let project_path = project_dir.path().join("config.toml");
    fs::write(&project_path, b"[cloud]\norg = \"proj\"\n").unwrap();

    let config = harmont_cli::config::Config::load_from_paths(
        None,
        Some(&project_path),
    )
    .unwrap();

    assert_eq!(config.cloud.org.as_deref(), Some("proj"));
    assert_eq!(config.cloud.api_url, "https://api.harmont.dev");
}
```

**Step 2: Run tests**

Run: `cargo test -p harmont-cli --test config_layered`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/hm/tests/config_layered.rs
git commit -m "test: integration tests for layered figment config"
```

---

### Task 8: Final verification — full workspace build & test

**Step 1: Build entire workspace**

Run: `cargo build`
Expected: PASS

**Step 2: Run all tests**

Run: `cargo test`
Expected: PASS

**Step 3: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: PASS

**Step 4: Verify binary works**

Run: `cargo run -- version`
Expected: prints version.

Run: `cargo run -- --help`
Expected: help output unchanged.

**Step 5: Final commit if any fixups needed**

```bash
git add -A
git commit -m "chore: fixups from full workspace verification"
```

---

## Summary of changes

| File | Action | What |
|------|--------|------|
| `Cargo.toml` (root) | Modify | Add `figment` workspace dep |
| `crates/hm/Cargo.toml` | Modify | Add `figment = { workspace = true }` |
| `crates/hm-util/src/dirs.rs` | Modify | Add `hm_user_config_dir()` and `find_project_root()` |
| `crates/hm/src/config.rs` | Rewrite | Figment-based `Config` with `CloudConfig` + `Preferences` sections |
| `crates/hm/src/context.rs` | Modify | Wire new `Config` + project root discovery |
| `crates/hm/tests/config_layered.rs` | Create | Integration tests for layered config |

**New dependency:** `figment` 0.10 (features: `toml`, `env`)

**Config file locations:**
- User: `~/.config/hm/config.toml`
- Project: `<project>/.harmont/config.toml`

**Merge precedence (low → high):** compiled defaults → user file → project file → env vars (`HM_CLOUD__ORG`, etc.)

**Breaking changes:**
- Old `~/.harmont/config.toml` no longer loaded — users must create `~/.config/hm/config.toml` with `[cloud]`/`[preferences]` sections
- `config.api_url()` method gone → `config.cloud.api_url` field
- `config.org` field → `config.cloud.org`
- Preferences fields no longer `Option` — resolved to concrete types with defaults
