// Build scripts legitimately panic on errors — no runtime to propagate to.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr
)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let workspace_root = manifest_dir.join("../..").canonicalize().unwrap();
    let ts_src = workspace_root.join("dsls/harmont-ts/src");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", ts_src.display());

    let esbuild = find_esbuild(&workspace_root);

    let Some(esbuild) = esbuild else {
        eprintln!(
            "cargo:warning=esbuild not found; writing stub bundles. \
             Run `npm ci` in dsls/harmont-ts/ for real bundles."
        );
        fs::write(out_dir.join("harmont-index.mjs"), "// stub\nexport {};").unwrap();
        fs::write(
            out_dir.join("harmont-toolchains.mjs"),
            "// stub\nexport {};",
        )
        .unwrap();
        return;
    };

    bundle(
        &esbuild,
        &ts_src.join("index.ts"),
        &out_dir.join("harmont-index.mjs"),
    );
    bundle(
        &esbuild,
        &ts_src.join("toolchains/index.ts"),
        &out_dir.join("harmont-toolchains.mjs"),
    );
}

fn bundle(esbuild: &Path, entry: &Path, outfile: &Path) {
    let status = Command::new(esbuild)
        .arg(entry)
        .arg("--bundle")
        .arg("--format=esm")
        .arg("--platform=node")
        .arg(format!("--outfile={}", outfile.display()))
        .status()
        .expect("failed to run esbuild");

    assert!(status.success(), "esbuild failed for {}", entry.display());
}

fn find_esbuild(workspace_root: &Path) -> Option<PathBuf> {
    let local = workspace_root.join("dsls/harmont-ts/node_modules/.bin/esbuild");
    if local.exists() {
        return Some(local);
    }
    let which = Command::new("which").arg("esbuild").output().ok()?;
    if which.status.success() {
        let path = String::from_utf8_lossy(&which.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    None
}
