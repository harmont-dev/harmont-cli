#!/bin/sh
# install.sh — one-line installer for the Harmont CLI (`hm`).
#
#   curl -fsSL https://get.harmont.dev/install.sh | sh
#
# Downloads the latest prebuilt `hm` binary for this platform from this repo's
# GitHub releases, verifies its SHA-256, and installs it to ~/.local/bin.
# Unsupported platforms are pointed at `cargo install`.
#
# This script is the source of truth for the installer and is versioned with
# the CLI: the supported targets below mirror the release matrix in
# .github/workflows/release.yml. Keep them in sync.
set -eu

BIN_NAME="hm"

err() { printf 'error: %s\n' "$*" >&2; }
die() { err "$*"; exit 1; }

uname_s() { printf '%s' "${HARMONT_UNAME_S:-$(uname -s)}"; }
uname_m() { printf '%s' "${HARMONT_UNAME_M:-$(uname -m)}"; }

REPO_RELEASES="${HARMONT_INSTALL_BASE_URL:-https://github.com/harmont-dev/harmont-cli/releases}"
VERSION="${HARMONT_INSTALL_VERSION:-latest}"

# release.yml names archives `hm-<target>.tar.gz`. Probe the binary name first
# (what we actually ship), then the package name as a defensive fallback so a
# future rename of the release asset doesn't silently break installs.
ASSET_PREFIXES="hm harmont-cli"

install_dir() { printf '%s' "${HARMONT_INSTALL_DIR:-${XDG_BIN_HOME:-$HOME/.local/bin}}"; }

# Build a download URL for an asset (handles "latest" vs a pinned tag).
asset_url() {
  if [ "$VERSION" = latest ]; then
    printf '%s/latest/download/%s' "$REPO_RELEASES" "$1"
  else
    printf '%s/download/%s/%s' "$REPO_RELEASES" "$VERSION" "$1"
  fi
}

# fetch URL DEST -> 0 on success. curl preferred, wget fallback. Supports file://.
fetch() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$1" -O "$2"
  else
    die "need curl or wget to download $1"
  fi
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    die "need sha256sum or shasum to verify the download"
  fi
}

# Map (os, arch) to a Rust target triple, or empty if unsupported. Linux uses
# musl (static) builds exclusively. Triples mirror the build-binary matrix in
# release.yml.
detect_target() {
  os="$(uname_s)"
  arch="$(uname_m)"
  case "$os" in
    Linux)
      case "$arch" in
        x86_64 | amd64)  printf 'x86_64-unknown-linux-musl' ;;
        aarch64 | arm64) printf 'aarch64-unknown-linux-musl' ;;
        *) printf '' ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        arm64 | aarch64) printf 'aarch64-apple-darwin' ;;
        x86_64)          printf 'x86_64-apple-darwin' ;;
        *) printf '' ;;
      esac
      ;;
    *) printf '' ;;
  esac
}

main() {
  target="$(detect_target)"
  if [ -z "$target" ]; then
    die "no prebuilt hm binary for $(uname_s)/$(uname_m).
Install from source instead:
    cargo install harmont-cli"
  fi

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  # Probe each candidate prefix until one downloads.
  archive=""
  for prefix in $ASSET_PREFIXES; do
    candidate="${prefix}-${target}.tar.gz"
    if fetch "$(asset_url "$candidate")" "$tmp/$candidate" 2>/dev/null; then
      archive="$candidate"
      break
    fi
  done
  [ -n "$archive" ] || die "could not download an hm release archive for $target from $REPO_RELEASES"

  # Checksum is mandatory — refuse to install an unverified binary.
  fetch "$(asset_url "$archive.sha256")" "$tmp/$archive.sha256" 2>/dev/null \
    || die "missing checksum for $archive — refusing to install unverified binary"
  expected="$(cut -d' ' -f1 < "$tmp/$archive.sha256")"
  actual="$(sha256_of "$tmp/$archive")"
  if [ "$expected" != "$actual" ]; then
    die "checksum mismatch for $archive
  expected $expected
  actual   $actual"
  fi

  tar -xzf "$tmp/$archive" -C "$tmp"
  bin_path="$(find "$tmp" -type f -name "$BIN_NAME" | head -n1)"
  [ -n "$bin_path" ] || die "archive $archive did not contain a '$BIN_NAME' binary"

  dest="$(install_dir)"
  mkdir -p "$dest"
  install -m 0755 "$bin_path" "$dest/$BIN_NAME" 2>/dev/null \
    || cp "$bin_path" "$dest/$BIN_NAME"
  chmod 0755 "$dest/$BIN_NAME" 2>/dev/null || true
  [ -x "$dest/$BIN_NAME" ] || die "failed to install $BIN_NAME to $dest"

  printf '%s\n' "✓ installed $BIN_NAME to $dest/$BIN_NAME"

  # PATH membership, glob-safe: quoting "$dest" inside the #-expansion makes it
  # a literal match, so an install dir containing glob metacharacters is fine.
  colonpath=":$PATH:"
  if [ "$colonpath" = "${colonpath#*":$dest:"}" ]; then
    printf '\n%s\n' "$dest is not on your PATH. Add it:"
    printf '    export PATH="%s:$PATH"\n' "$dest"
  fi
  printf "Run 'hm --help' to get started.\n"
}

main "$@"
