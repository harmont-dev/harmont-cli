use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tracing::debug;

use crate::bundled_sources;
use crate::{DslEngine, PipelineMeta};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsRuntime {
    Bun,
    Node,
}

impl JsRuntime {
    fn detect() -> Result<(Self, std::path::PathBuf)> {
        if let Ok(p) = which::which("bun") {
            return Ok((Self::Bun, p));
        }
        if let Ok(p) = which::which("node") {
            return Ok((Self::Node, p));
        }
        bail!(
            "no JavaScript runtime found on PATH\n  \
             → install Bun (https://bun.sh) or Node.js 22+ (https://nodejs.org)"
        )
    }
}

const RUNNER_SCRIPT: &str = r#"
import { readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

const projectDir = process.argv[2];
const mode = process.argv[3];       // "list" or "render"
const slug = process.argv[4] || null;
const harmontDir = join(projectDir, '.hm');

const tsFiles = readdirSync(harmontDir)
  .filter(f => f.endsWith('.ts'))
  .sort();

if (tsFiles.length === 0) {
  process.stderr.write(`error: no .ts files found in ${harmontDir}\n`);
  process.exit(1);
}

const defs = [];
for (const file of tsFiles) {
  const filePath = resolve(harmontDir, file);
  const mod = await import(filePath);
  const d = mod.default ?? mod.pipelines;
  if (Array.isArray(d)) defs.push(...d);
  else if (d) defs.push(d);
}

const { renderEnvelope } = await import('@harmont/hm');
const envelope = JSON.parse(renderEnvelope(defs, { basePath: projectDir }));

if (mode === 'render') {
  const match = envelope.pipelines.find(p => p.slug === slug);
  if (!match) {
    const avail = envelope.pipelines.map(p => p.slug).join(', ') || '(none)';
    process.stderr.write(`error: pipeline '${slug}' not found\n  -> available: ${avail}\n`);
    process.exit(2);
  }
  process.stdout.write(JSON.stringify(match.definition));
} else {
  const metas = envelope.pipelines.map(p => ({ slug: p.slug, name: p.name }));
  process.stdout.write(JSON.stringify(metas));
}
"#;

const PACKAGE_JSON: &str = r#"{"name":"@harmont/hm","type":"module","exports":{".":"./index.mjs","./toolchains":"./toolchains.mjs"}}"#;

struct SymlinkCleanup {
    pkg: std::path::PathBuf,
    nm: std::path::PathBuf,
    remove_nm: bool,
}

impl Drop for SymlinkCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.pkg).or_else(|_| std::fs::remove_dir_all(&self.pkg));
        // The scoped package lives under an intermediate `@harmont/` scope dir
        // (`.hm/node_modules/@harmont/hm`). After removing the symlink, prune the
        // now-empty scope dir (best-effort), then the node_modules dir.
        if let Some(scope) = self.pkg.parent() {
            let _ = std::fs::remove_dir(scope);
        }
        if self.remove_nm {
            let _ = std::fs::remove_dir(&self.nm);
        }
    }
}

#[derive(Debug)]
pub struct SubprocessTsEngine {
    runtime: JsRuntime,
    runtime_bin: std::path::PathBuf,
}

impl SubprocessTsEngine {
    /// Create engine, detecting the preferred JS runtime (`bun` or `node`).
    ///
    /// # Errors
    ///
    /// Returns an error if neither `bun` nor `node` is found on `PATH`.
    pub fn new() -> Result<Self> {
        let (runtime, runtime_bin) = JsRuntime::detect()?;
        debug!(?runtime, ?runtime_bin, "detected JS runtime");
        Ok(Self {
            runtime,
            runtime_bin,
        })
    }

    #[allow(clippy::unused_self)] // method for consistency with engine API
    fn setup_temp(&self) -> Result<tempfile::TempDir> {
        let tmp = tempfile::tempdir().context("creating temp dir for harmont-ts")?;

        let pkg_dir = tmp.path().join("node_modules/@harmont/hm");
        std::fs::create_dir_all(&pkg_dir).context("creating node_modules/@harmont/hm")?;

        std::fs::write(pkg_dir.join("package.json"), PACKAGE_JSON)?;
        std::fs::write(pkg_dir.join("index.mjs"), bundled_sources::HARMONT_TS_INDEX)?;
        std::fs::write(
            pkg_dir.join("toolchains.mjs"),
            bundled_sources::HARMONT_TS_TOOLCHAINS,
        )?;

        std::fs::write(tmp.path().join("runner.mjs"), RUNNER_SCRIPT)?;

        Ok(tmp)
    }

    fn should_create_symlink(local_pkg: &Path) -> bool {
        match local_pkg.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                // Stale symlink from previous run — remove so we can recreate
                let _ = std::fs::remove_file(local_pkg);
                true
            }
            Ok(_) => {
                // Real directory (npm-installed package) — leave it alone
                false
            }
            Err(_) => {
                // Doesn't exist — create symlink
                true
            }
        }
    }

    async fn run(&self, project_dir: &Path, mode: &str, slug: Option<&str>) -> Result<String> {
        let tmp = self.setup_temp()?;
        let runner_path = tmp.path().join("runner.mjs");

        // Node ESM resolves bare specifiers relative to the importing file,
        // ignoring NODE_PATH.  User .ts files live under <project>/.hm/,
        // so we place a node_modules/@harmont/hm symlink there so
        // `import '@harmont/hm'` resolves.  Cleaned up after the subprocess
        // finishes.
        let harmont_dir = project_dir.join(".hm");
        let local_nm = harmont_dir.join("node_modules");
        let local_pkg = local_nm.join("@harmont/hm");

        let _cleanup: Option<SymlinkCleanup> = if Self::should_create_symlink(&local_pkg) {
            let created_local_nm = !local_nm.exists();

            // Create the `@harmont/` scope dir (and node_modules) before
            // symlinking the scoped package into it.
            if let Some(scope_dir) = local_pkg.parent() {
                std::fs::create_dir_all(scope_dir)
                    .context("creating .hm/node_modules/@harmont for module resolution")?;
            }

            let src = tmp.path().join("node_modules/@harmont/hm");

            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&src, &local_pkg)
                    .context("symlinking @harmont/hm package into .hm/node_modules")?;
            }
            #[cfg(not(unix))]
            {
                // Fallback: copy files for non-unix platforms.
                std::fs::create_dir_all(&local_pkg)?;
                for entry in std::fs::read_dir(&src)? {
                    let entry = entry?;
                    std::fs::copy(entry.path(), local_pkg.join(entry.file_name()))?;
                }
            }

            Some(SymlinkCleanup {
                pkg: local_pkg.clone(),
                nm: local_nm.clone(),
                remove_nm: created_local_nm,
            })
        } else {
            debug!(?local_pkg, "npm-installed @harmont/hm found — skipping symlink");
            None
        };

        let mut cmd = tokio::process::Command::new(&self.runtime_bin);

        match self.runtime {
            JsRuntime::Bun => {
                cmd.arg("run").arg(&runner_path);
            }
            JsRuntime::Node => {
                cmd.arg("--experimental-strip-types").arg(&runner_path);
            }
        }

        cmd.arg(project_dir).arg(mode);

        if let Some(s) = slug {
            cmd.arg(s);
        }

        cmd.env("NODE_PATH", tmp.path().join("node_modules"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        debug!(?cmd, "running JS subprocess");

        let output = cmd.output().await.context("spawning JS runtime")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let code = output.status.code().unwrap_or(-1);
            bail!("{:?} exited with code {code}:\n{stderr}", self.runtime);
        }

        String::from_utf8(output.stdout).context("JS runtime stdout is not valid UTF-8")
    }
}

#[async_trait]
impl DslEngine for SubprocessTsEngine {
    async fn list_pipelines(&self, project_dir: &Path) -> Result<Vec<PipelineMeta>> {
        let stdout = self
            .run(project_dir, "list", None)
            .await
            .context("listing pipelines via JS runtime")?;

        debug!(raw_len = stdout.len(), "list_pipelines stdout");

        serde_json::from_str(&stdout).context("decoding pipeline metadata from JS stdout")
    }

    async fn render_pipeline_json(&self, project_dir: &Path, slug: &str) -> Result<String> {
        self.run(project_dir, "render", Some(slug))
            .await
            .context("rendering pipeline via JS runtime")
    }

    async fn registry_json(&self, _project_dir: &Path) -> Result<String> {
        bail!(
            "the discovery envelope (hm pipelines) is not yet supported for \
             TypeScript pipelines; only Python pipelines are supported today"
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn symlink_skipped_when_real_dir_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let harmont_dir = tmp.path().join(".hm");
        let nm = harmont_dir.join("node_modules");
        let pkg = nm.join("@harmont/hm");

        // Simulate npm-installed package (real directory)
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("package.json"), "{}").unwrap();

        assert!(!SubprocessTsEngine::should_create_symlink(&pkg));
    }

    #[test]
    fn symlink_created_when_nothing_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join("node_modules/@harmont/hm");
        assert!(SubprocessTsEngine::should_create_symlink(&pkg));
    }

    #[test]
    fn symlink_created_when_stale_symlink_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join("node_modules/@harmont/hm");
        std::fs::create_dir_all(pkg.parent().unwrap()).unwrap();

        // Create a dangling symlink
        #[cfg(unix)]
        std::os::unix::fs::symlink("/nonexistent", &pkg).unwrap();

        assert!(SubprocessTsEngine::should_create_symlink(&pkg));
    }
}
