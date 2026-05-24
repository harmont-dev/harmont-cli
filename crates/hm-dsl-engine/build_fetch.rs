use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

pub(crate) struct PinnedAsset {
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
}

pub(crate) fn ensure_asset(asset: &PinnedAsset, out_dir: &Path) -> PathBuf {
    let dest = out_dir.join(asset.name);

    // If already present, verify checksum — skip download on match.
    if dest.exists() {
        let existing = fs::read(&dest).expect("failed to read cached asset");
        if verify(&existing, asset.sha256) {
            return dest;
        }
        println!(
            "cargo:warning={}: SHA-256 mismatch in cache, re-downloading",
            asset.name
        );
    }

    println!("cargo:warning=downloading {} from {}", asset.name, asset.url);

    let response = reqwest::blocking::get(asset.url)
        .unwrap_or_else(|e| panic!("failed to download {}: {e}", asset.url));

    let bytes = response
        .bytes()
        .unwrap_or_else(|e| panic!("failed to read response body for {}: {e}", asset.url));

    if !verify(&bytes, asset.sha256) {
        panic!(
            "SHA-256 mismatch for {}:\n  expected: {}\n  got:      {}",
            asset.name,
            asset.sha256,
            hex::encode(Sha256::digest(&bytes)),
        );
    }

    fs::write(&dest, &bytes)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", dest.display()));

    println!(
        "cargo:warning={}: downloaded {} bytes, SHA-256 verified",
        asset.name,
        bytes.len()
    );

    dest
}

pub(crate) fn extract_sdist_package(
    tarball_path: &Path,
    package_name: &str,
    dest_dir: &Path,
) -> usize {
    let file =
        fs::File::open(tarball_path).unwrap_or_else(|e| panic!("open tarball: {e}"));
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    // Pattern: <sdist-toplevel>/<package_name>/...
    // We strip the sdist top-level directory and write to dest_dir/<package_name>/...
    let needle = format!("/{package_name}/");
    let mut count = 0usize;

    for entry in archive.entries().expect("failed to read tar entries") {
        let mut entry = entry.expect("bad tar entry");
        let path = entry
            .path()
            .expect("non-utf8 tar path")
            .to_path_buf();
        let path_str = path.to_string_lossy();

        // Match entries whose path contains `/<package_name>/`.
        if let Some(pos) = path_str.find(&needle) {
            // The relative portion starting from `<package_name>/...`
            let rel = &path_str[pos + 1..]; // strip leading '/'

            let out_path = dest_dir.join(rel);

            if entry.header().entry_type().is_dir() {
                fs::create_dir_all(&out_path)
                    .unwrap_or_else(|e| panic!("mkdir {}: {e}", out_path.display()));
            } else {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)
                        .unwrap_or_else(|e| panic!("mkdir {}: {e}", parent.display()));
                }
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf).expect("read tar entry");
                fs::write(&out_path, &buf)
                    .unwrap_or_else(|e| panic!("write {}: {e}", out_path.display()));
                count += 1;
            }
        }
    }

    count
}

pub(crate) fn extract_sdist_single_file(tarball_path: &Path, filename: &str) -> Vec<u8> {
    let file =
        fs::File::open(tarball_path).unwrap_or_else(|e| panic!("open tarball: {e}"));
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    // Match `<anything>/<filename>` (one level of sdist top-dir).
    for entry in archive.entries().expect("failed to read tar entries") {
        let mut entry = entry.expect("bad tar entry");
        let path = entry.path().expect("non-utf8 tar path").to_path_buf();

        if let Some(name) = path.file_name() {
            if name == filename {
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf).expect("read tar entry");
                return buf;
            }
        }
    }

    panic!(
        "file `{filename}` not found in tarball {}",
        tarball_path.display()
    );
}

fn verify(bytes: &[u8], expected_hex: &str) -> bool {
    let digest = Sha256::digest(bytes);
    let actual = hex::encode(digest);
    actual.eq_ignore_ascii_case(expected_hex)
}
