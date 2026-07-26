//! Running git.

use std::path::{Path, PathBuf};
use std::process::Command;

use bstr::{BStr, BString, ByteSlice};

use crate::process::{CapturedStreams as _, CommandExt as _};

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

    /// The commit the branch points at, as a hex object id. `None` if git fails.
    #[tracing::instrument(skip(self))]
    pub fn head_commit(&self) -> Option<BString> {
        let name = self.name.to_str().ok()?;
        self.repo.run(&["rev-parse", name])
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
    use rstest::rstest;

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
        assert_eq!(branch.head_commit().unwrap().len(), 40);

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
