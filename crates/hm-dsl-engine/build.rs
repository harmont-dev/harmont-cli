#[path = "build_fetch.rs"]
mod build_fetch;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let ts_enabled = env::var("CARGO_FEATURE_EMBEDDED_TYPESCRIPT").is_ok();
    let py_enabled = env::var("CARGO_FEATURE_EMBEDDED_PYTHON").is_ok();

    if ts_enabled {
        // ------------------------------------------------------------------
        // QuickJS WASM binary
        // ------------------------------------------------------------------
        const QUICKJS: build_fetch::PinnedAsset = build_fetch::PinnedAsset {
            name: "quickjs.wasm",
            url: "https://github.com/harmont-dev/harmont-cli/releases/download/assets/quickjs-wasm-v1/quickjs.wasm",
            sha256: "1ced88f9fc8e8b782814e4e01af1a4d7d38998c911a8f5914ca7a609923a2fe1",
        };
        build_fetch::ensure_asset(&QUICKJS, &out_dir);
    }

    if py_enabled {
        // ------------------------------------------------------------------
        // Python vendor packages from PyPI
        // ------------------------------------------------------------------
        let vendor_dir = out_dir.join("vendor");

        const CRONITER: build_fetch::PinnedAsset = build_fetch::PinnedAsset {
            name: "croniter-3.0.4.tar.gz",
            url: "https://files.pythonhosted.org/packages/source/c/croniter/croniter-3.0.4.tar.gz",
            sha256: "f9dcd4bdb6c97abedb6f09d6ed3495b13ede4d4544503fa580b6372a56a0c520",
        };
        let croniter_tarball = build_fetch::ensure_asset(&CRONITER, &out_dir);
        build_fetch::extract_sdist_package(&croniter_tarball, "croniter", &vendor_dir);

        const PYTHON_DATEUTIL: build_fetch::PinnedAsset = build_fetch::PinnedAsset {
            name: "python-dateutil-2.9.0.post0.tar.gz",
            url: "https://files.pythonhosted.org/packages/66/c0/0c8b6ad9f17a802ee498c46e004a0eb49bc148f2fd230864601a86dcf6db/python-dateutil-2.9.0.post0.tar.gz",
            sha256: "37dd54208da7e1cd875388217d5e00ebd4179249f90fb72437e91a35459a0ad3",
        };
        let dateutil_tarball = build_fetch::ensure_asset(&PYTHON_DATEUTIL, &out_dir);
        build_fetch::extract_sdist_package(&dateutil_tarball, "dateutil", &vendor_dir);

        const SIX: build_fetch::PinnedAsset = build_fetch::PinnedAsset {
            name: "six-1.17.0.tar.gz",
            url: "https://files.pythonhosted.org/packages/source/s/six/six-1.17.0.tar.gz",
            sha256: "ff70335d468e7eb6ec65b95b99d3a2836546063f63acc5171de367e834932a81",
        };
        let six_tarball = build_fetch::ensure_asset(&SIX, &out_dir);
        let six_bytes = build_fetch::extract_sdist_single_file(&six_tarball, "six.py");
        fs::create_dir_all(&vendor_dir).expect("create vendor dir");
        fs::write(vendor_dir.join("six.py"), &six_bytes).expect("write six.py");

        println!("cargo:rustc-env=VENDOR_DIR={}", vendor_dir.display());
    }

    if !ts_enabled {
        return;
    }

    let workspace_root = manifest_dir.join("../..").canonicalize().unwrap();

    let ts_src_dir = workspace_root.join("dsls/harmont-ts/src");
    let polyfill_path = manifest_dir.join("embedded/sha256-polyfill.js");

    // Re-run if source files change.
    println!("cargo:rerun-if-changed={}", ts_src_dir.display());
    println!("cargo:rerun-if-changed={}", polyfill_path.display());

    let bundle_out = out_dir.join("harmont-bundle.js");

    let polyfill = fs::read_to_string(&polyfill_path)
        .expect("failed to read sha256-polyfill.js");

    // Try to find esbuild.
    let esbuild_bin = find_esbuild(&workspace_root);

    let Some(esbuild) = esbuild_bin else {
        eprintln!(
            "cargo:warning=esbuild not found; writing stub harmont-bundle.js. \
             Install esbuild or run `npm install` in dsls/harmont-ts/ for a real bundle."
        );
        fs::write(
            &bundle_out,
            "// stub: esbuild was not available at build time\nvar harmont = {};\n",
        )
        .unwrap();
        return;
    };

    // Create a crypto shim that delegates to globalThis.createHash (provided
    // by the SHA-256 polyfill prepended to the bundle).
    let shim_path = out_dir.join("_crypto_shim.ts");
    fs::write(
        &shim_path,
        "export function createHash(algo: string) {\n  \
         return (globalThis as any).createHash(algo);\n}\n",
    )
    .unwrap();

    // Run esbuild to bundle the TS DSL into a single IIFE.
    let entry = ts_src_dir.join("index.ts");
    let raw_bundle_path = out_dir.join("harmont-raw.js");

    let status = Command::new(&esbuild)
        .arg(entry.to_str().unwrap())
        .arg("--bundle")
        .arg("--format=iife")
        .arg("--global-name=harmont")
        .arg("--platform=neutral")
        .arg(format!("--alias:node:crypto={}", shim_path.display()))
        .arg(format!("--outfile={}", raw_bundle_path.display()))
        .status()
        .expect("failed to run esbuild");

    if !status.success() {
        panic!("esbuild failed with status {status}");
    }

    let raw_bundle =
        fs::read_to_string(&raw_bundle_path).expect("failed to read esbuild output");

    // Prepend polyfill to the bundle.
    let combined = format!(
        "// === SHA-256 polyfill ===\n{polyfill}\n// === harmont-ts bundle ===\n{raw_bundle}"
    );
    fs::write(&bundle_out, combined).unwrap();

    println!("cargo:warning=harmont-ts bundle written to {}", bundle_out.display());
}

/// Locate an esbuild binary. Checks the local node_modules first, then PATH.
fn find_esbuild(workspace_root: &Path) -> Option<PathBuf> {
    // 1. Local node_modules binary in dsls/harmont-ts/
    let local = workspace_root.join("dsls/harmont-ts/node_modules/.bin/esbuild");
    if local.exists() {
        return Some(local);
    }

    // 2. System PATH
    let which = Command::new("which").arg("esbuild").output().ok()?;
    if which.status.success() {
        let path = String::from_utf8_lossy(&which.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }

    None
}
