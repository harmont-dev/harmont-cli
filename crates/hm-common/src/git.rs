//! Git integration: running the `git` CLI and git value types.

use std::path::{Path, PathBuf};
use std::process::Command;

use bstr::{BStr, BString, ByteSlice};

use crate::process::{CapturedStreams as _, CommandExt as _};

/// A git object identifier: a SHA-1 digest, stored as its 20 raw bytes and
/// rendered as 40 lowercase hex characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GitSha([u8; 20]);

/// A string that is not a valid [`GitSha`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GitShaError {
    /// The digest is not 40 hex characters long.
    #[error("git sha must be 40 hex chars, got {0}")]
    BadLength(usize),
    /// The digest contains a non-hex character.
    #[error("git sha contains a non-hex character")]
    NotHex,
}

impl GitSha {
    /// The all-zero null oid git uses to mean "no commit".
    #[must_use]
    pub const fn zero() -> Self {
        Self([0; 20])
    }

    /// Whether this is the all-zero null oid git uses to mean "no commit".
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == [0; 20]
    }
}

impl std::str::FromStr for GitSha {
    type Err = GitShaError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 40 {
            return Err(GitShaError::BadLength(s.len()));
        }
        let mut bytes = [0; 20];
        hex::decode_to_slice(s, &mut bytes).map_err(|_| GitShaError::NotHex)?;
        Ok(Self(bytes))
    }
}

impl TryFrom<&str> for GitSha {
    type Error = GitShaError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl TryFrom<String> for GitSha {
    type Error = GitShaError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl std::fmt::Display for GitSha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl serde::Serialize for GitSha {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for GitSha {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// A path that is not a git repository.
#[derive(Debug, thiserror::Error)]
#[error("`{path}` is not a git repository")]
pub struct InvalidRepoError {
    path: PathBuf,
}

/// A resolved `git` executable.
#[derive(Debug, Clone, Copy)]
pub struct Git<'bin> {
    bin: &'bin Path,
}

impl<'bin> Git<'bin> {
    /// Wrap a `git` executable, typically via `AppCtx::git()`.
    #[must_use]
    pub const fn new(bin: &'bin Path) -> Self {
        Self { bin }
    }

    /// Bind to the git work tree at `repo`.
    ///
    /// # Errors
    /// [`InvalidRepoError`] if `repo` is not inside a git work tree.
    #[tracing::instrument(skip(self))]
    pub fn repo<'g>(&'g self, repo: &'g Path) -> Result<GitRepo<'g, 'bin>, InvalidRepoError> {
        let bound = GitRepo { git: self, repo };
        if bound.run(&["rev-parse", "--git-dir"]).is_some() {
            Ok(bound)
        } else {
            Err(InvalidRepoError {
                path: repo.to_path_buf(),
            })
        }
    }
}

/// A git work tree, bound to a [`Git`] and a repository path.
#[derive(Debug, Clone, Copy)]
pub struct GitRepo<'g, 'bin> {
    git: &'g Git<'bin>,
    repo: &'g Path,
}

impl<'g, 'bin> GitRepo<'g, 'bin> {
    /// Run `git -C <repo> <args>`, returning trimmed stdout on a zero exit.
    /// `None` on spawn failure, a non-zero exit, or empty output.
    fn run(&self, args: &[&str]) -> Option<BString> {
        let mut cmd = Command::new(self.git.bin);
        cmd.arg("-C").arg(self.repo).args(args);
        let out = cmd.captured().ok()?.success().ok()?;
        let trimmed = out.stdout().trim();
        (!trimmed.is_empty()).then(|| BString::from(trimmed))
    }

    /// The checked-out branch (`HEAD` when detached). `None` if git fails.
    #[tracing::instrument(skip(self))]
    pub fn current_branch(&self) -> Option<GitBranch<'_, 'g, 'bin>> {
        let name = self.run(&["rev-parse", "--abbrev-ref", "HEAD"])?;
        Some(GitBranch { repo: self, name })
    }

    /// The named remote, or `None` when it is not configured.
    #[tracing::instrument(skip(self))]
    pub fn remote(&self, name: &str) -> Option<GitRemote<'_, 'g, 'bin>> {
        let url = self.run(&["config", "--get", &format!("remote.{name}.url")])?;
        Some(GitRemote {
            repo: self,
            name: name.to_owned(),
            url,
        })
    }
}

/// A branch in a [`GitRepo`].
#[derive(Debug, Clone)]
pub struct GitBranch<'r, 'g, 'bin> {
    repo: &'r GitRepo<'g, 'bin>,
    name: BString,
}

impl<'r, 'g, 'bin> GitBranch<'r, 'g, 'bin> {
    /// The branch name (e.g. `main`, or `HEAD` when detached).
    #[must_use]
    pub fn name(&self) -> &BStr {
        self.name.as_bstr()
    }

    /// The commit the branch points at. `None` if git fails or its output is
    /// not a valid object id.
    #[tracing::instrument(skip(self))]
    pub fn head_commit(&self) -> Option<GitSha> {
        let name = self.name.to_str().ok()?;
        self.repo.run(&["rev-parse", name])?.to_str().ok()?.parse().ok()
    }
}

/// A remote of a [`GitRepo`].
#[derive(Debug, Clone)]
pub struct GitRemote<'r, 'g, 'bin> {
    repo: &'r GitRepo<'g, 'bin>,
    name: String,
    url: BString,
}

impl<'r, 'g, 'bin> GitRemote<'r, 'g, 'bin> {
    /// The remote's name (e.g. `origin`).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The remote's configured URL.
    #[must_use]
    pub fn url(&self) -> &BStr {
        self.url.as_bstr()
    }

    /// `owner/repo` parsed from the remote URL, mirroring the backend's
    /// `Harmont.Pipelines.RepoName`. `None` when fewer than two path segments
    /// remain.
    #[must_use]
    pub fn gh_repo_name(&self) -> Option<BString> {
        parse_gh_repo_name(self.url.as_bstr())
    }

    /// The remote's default branch, from its `HEAD` symbolic ref. `None` when
    /// `<remote>/HEAD` is unset (common on fresh clones).
    #[tracing::instrument(skip(self))]
    pub fn default_branch(&self) -> Option<GitBranch<'r, 'g, 'bin>> {
        let line = self
            .repo
            .run(&["symbolic-ref", &format!("refs/remotes/{}/HEAD", self.name)])?;
        let name = parse_default_branch(line.as_bstr(), &self.name)?;
        Some(GitBranch {
            repo: self.repo,
            name,
        })
    }
}

/// Extract `owner/repo` from a remote URL: drop scheme/host and a trailing
/// `.git`, then take the last two non-empty path segments.
fn parse_gh_repo_name(url: &BStr) -> Option<BString> {
    let url = url.to_str().ok()?.trim();
    let path = if let Some((_, rest)) = url.split_once("://") {
        rest.split_once('/').map_or(rest, |(_, p)| p)
    } else if url.contains('@') && url.contains(':') {
        let after_at = url.split_once('@').map_or(url, |(_, r)| r);
        after_at.split_once(':').map_or(after_at, |(_, p)| p)
    } else {
        url.split_once('/').map_or(url, |(_, p)| p)
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        return None;
    }
    Some(BString::from(segs[segs.len() - 2..].join("/")))
}

/// Extract the branch name from a `symbolic-ref refs/remotes/<remote>/HEAD`
/// result (e.g. `refs/remotes/origin/main` → `main`).
fn parse_default_branch(line: &BStr, remote: &str) -> Option<BString> {
    let line = line.to_str().ok()?.trim();
    let branch = line.strip_prefix(&format!("refs/remotes/{remote}/"))?;
    (!branch.is_empty()).then(|| BString::from(branch))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test setup and assertions")]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rstest::rstest;

    #[rstest]
    #[case::lower("0123456789abcdef0123456789abcdef01234567")]
    #[case::upper("0123456789ABCDEF0123456789ABCDEF01234567")]
    fn parses_and_renders_lowercase(#[case] input: &str) {
        let sha: GitSha = input.parse().unwrap();
        assert_eq!(sha.to_string(), input.to_ascii_lowercase());
    }

    #[rstest]
    #[case::empty(0)]
    #[case::short(39)]
    #[case::just_over(41)]
    #[case::sha256_width(64)]
    fn rejects_wrong_length(#[case] len: usize) {
        assert_eq!("a".repeat(len).parse::<GitSha>(), Err(GitShaError::BadLength(len)));
    }

    #[rstest]
    fn rejects_non_hex() {
        let with_g = format!("{}g", "a".repeat(39));
        assert_eq!(with_g.parse::<GitSha>(), Err(GitShaError::NotHex));
    }

    #[rstest]
    fn zero_is_the_null_oid() {
        let sha: GitSha = "0".repeat(40).parse().unwrap();
        assert!(sha.is_zero());
        assert_eq!(sha, GitSha::zero());
    }

    #[rstest]
    fn non_zero_is_not_the_null_oid() {
        let sha: GitSha = "0123456789abcdef0123456789abcdef01234567".parse().unwrap();
        assert!(!sha.is_zero());
    }

    #[rstest]
    fn serde_round_trips_as_a_bare_string() {
        let sha: GitSha = "0123456789abcdef0123456789abcdef01234567".parse().unwrap();
        let json = serde_json::to_string(&sha).unwrap();
        assert_eq!(json, "\"0123456789abcdef0123456789abcdef01234567\"");
        assert_eq!(serde_json::from_str::<GitSha>(&json).unwrap(), sha);
    }

    #[rstest]
    fn deserialize_rejects_an_invalid_digest() {
        assert!(serde_json::from_str::<GitSha>("\"not-a-sha\"").is_err());
    }

    proptest! {
        #[test]
        fn every_digest_round_trips_through_hex(bytes in any::<[u8; 20]>()) {
            let sha = GitSha(bytes);
            let hex = sha.to_string();
            prop_assert_eq!(hex.len(), 40);
            prop_assert_eq!(hex.parse::<GitSha>().unwrap(), sha);
        }
    }

    #[rstest]
    #[case::https("https://github.com/acme/web.git", Some("acme/web"))]
    #[case::https_no_suffix("https://github.com/acme/web", Some("acme/web"))]
    #[case::scp("git@github.com:acme/web.git", Some("acme/web"))]
    #[case::ssh("ssh://git@github.com/acme/web", Some("acme/web"))]
    #[case::trailing_slash("https://github.com/acme/web/", Some("acme/web"))]
    #[case::nested("https://gitlab.com/group/sub/web.git", Some("sub/web"))]
    #[case::deep_path("https://example.com/a/b/c/repo", Some("c/repo"))]
    #[case::too_short("https://github.com/web", None)]
    #[case::empty("", None)]
    #[case::not_a_url("not-a-url", None)]
    fn parses_gh_repo_name(#[case] url: &str, #[case] expected: Option<&str>) {
        let got = parse_gh_repo_name(url.into());
        assert_eq!(got, expected.map(BString::from));
    }

    #[rstest]
    #[case::main("refs/remotes/origin/main", "origin", Some("main"))]
    #[case::trailing_newline("refs/remotes/origin/main\n", "origin", Some("main"))]
    #[case::slash("refs/remotes/origin/feature/x", "origin", Some("feature/x"))]
    #[case::other_remote("refs/remotes/upstream/dev", "upstream", Some("dev"))]
    #[case::wrong_prefix("refs/heads/main", "origin", None)]
    #[case::trailing_slash("refs/remotes/origin/", "origin", None)]
    #[case::empty("", "origin", None)]
    fn parses_default_branch(
        #[case] line: &str,
        #[case] remote: &str,
        #[case] expected: Option<&str>,
    ) {
        let got = parse_default_branch(line.into(), remote);
        assert_eq!(got, expected.map(BString::from));
    }

    /// A throwaway git repo with one commit and an `origin` remote.
    fn temp_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let ok = Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .captured()
                .unwrap()
                .success();
            assert!(ok.is_ok(), "git {args:?} failed");
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@t.dev"]);
        git(&["config", "user.name", "t"]);
        git(&["commit", "-q", "--allow-empty", "-m", "init"]);
        git(&["remote", "add", "origin", "git@github.com:acme/web.git"]);
        dir
    }

    #[rstest]
    fn repo_rejects_a_non_repo_dir() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(Path::new("git"));
        assert!(git.repo(dir.path()).is_err());
    }

    #[rstest]
    fn reads_branch_commit_and_remote() {
        let dir = temp_repo();
        let git = Git::new(Path::new("git"));
        let repo = git.repo(dir.path()).unwrap();

        let branch = repo.current_branch().unwrap();
        assert_eq!(branch.name(), "main");
        assert!(!branch.head_commit().unwrap().is_zero());

        let remote = repo.remote("origin").unwrap();
        assert_eq!(remote.name(), "origin");
        assert_eq!(remote.gh_repo_name(), Some(BString::from("acme/web")));
    }

    #[rstest]
    fn default_branch_reads_origin_head() {
        let dir = temp_repo();
        let git = Git::new(Path::new("git"));
        let repo = git.repo(dir.path()).unwrap();
        // Point origin/HEAD at a fabricated remote-tracking ref.
        Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args([
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ])
            .captured()
            .unwrap()
            .success()
            .unwrap();

        let branch = repo.remote("origin").unwrap().default_branch().unwrap();
        assert_eq!(branch.name(), "main");
    }
}
