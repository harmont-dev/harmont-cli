# Releasing harmont (Python DSL)

The `harmont` Python package lives at `dsls/harmont-py/` in the
harmont-cli monorepo. It is published to PyPI alongside the Rust
crates when a version tag is pushed.

## Cutting a release

Releases are driven by git tags on this repo. The release workflow
(`.github/workflows/release.yml`) triggers on any tag matching `v*`,
seds the version into `dsls/harmont-py/pyproject.toml`, builds the
sdist and wheel, and publishes to PyPI via Trusted Publishing (OIDC).

1. Tag from the repo root:

   ```sh
   git tag v<version>
   git push origin v<version>
   ```

2. Watch the run:

   ```sh
   gh run watch \
     "$(gh run list --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId')" \
     --exit-status
   ```

3. Confirm on <https://pypi.org/project/harmont/>.

## PyPI Trusted Publisher setup

Configure on <https://pypi.org/manage/project/harmont/settings/publishing/>:
- Owner: `harmont-dev`
- Repository: `harmont-cli` (this repo, not the archived harmont-py)
- Workflow filename: `release.yml`
- Environment: `release`
