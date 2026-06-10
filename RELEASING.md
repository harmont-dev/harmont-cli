# Releasing harmont-cli

This repo is the **public mirror** of the monorepo's `cli/` workspace and
top-level `examples/` directory. The monorepo is the source of truth; do not
land changes here directly.

## How the mirror is synced

On every push to `main` in the monorepo that touches `cli/**` or `examples/**`,
GitHub Actions runs `scripts/sync-cli-mirror.sh`. That script:

1. Calls `scripts/build-cli-mirror.sh` to assemble the export tree in a
   temporary directory. Internal files (`CLAUDE.md`, references to the
   monorepo's `PRINCIPLES.md`) are stripped at this step.
2. Initializes a fresh git repo inside the tree, commits the snapshot with
   message `Sync from harmont monorepo @ <SHA>`, and force-pushes it to
   `harmont-cli` main.

The mirror's history is intentionally a sequence of snapshot commits, not
a replay of monorepo commits. To pin downstream consumers, use a tag (see
below) or a specific mirror SHA.

## Forcing a manual sync

From the monorepo root, with `MIRROR_SSH_KEY` set to the path of the deploy
private key:

```sh
MIRROR_SSH_KEY=~/.ssh/harmont-cli-deploy ./scripts/sync-cli-mirror.sh
```

This is the same command CI runs. Use it after the automation fails or to
test changes to the export script.

## Cutting a release

Versioning is **driven by git tags on the public mirror**. The release
workflow in `.github/workflows/release.yml` triggers on any tag matching
`v*`, seds the version from the tag into every publishable crate's
`Cargo.toml` plus the `workspace.dependencies` pins, runs
`cargo package --workspace` as a fail-fast guard (it resolves sibling
path deps locally, so it catches the `publish = false` /
unpublished-dep class of regression without needing the deps on the
index yet — which `cargo publish --dry-run` would), then publishes the
crates to crates.io in dependency (topological) order:

```
hm-util → hm-pipeline-ir → hm-config → hm-plugin-protocol → hm-render
→ hm-vm → hm-exec → hm-plugin-cloud → hm-dsl-engine → harmont-cli
```

`harmont-cli` (the `hm` binary) depends on every other crate, so they
must all reach crates.io first — including `hm-vm`, which carries the
local Docker backend. `hm-fixtures` is test-only and not published.

### Prerequisites (one-time)

- `CRATES_IO_TOKEN` set as a repository secret on
  https://github.com/harmont-dev/harmont-cli/settings/secrets/actions.
  Generate it from https://crates.io/me. The first release needs the
  `publish-new` scope (the crates do not yet exist on crates.io); after
  the initial publish, narrow the token to `publish-update`.

### Per-release procedure

1. Land all the changes that go into the release on monorepo `main`.
2. Wait for the next `cli-mirror` sync to land the latest contents on
   the public mirror.
3. Tag the **mirror** (not the monorepo) and push the tag:

   ```sh
   git clone https://github.com/harmont-dev/harmont-cli /tmp/cli-release
   cd /tmp/cli-release
   git tag v1.2.3
   git push origin v1.2.3
   ```

4. Watch the workflow at
   https://github.com/harmont-dev/harmont-cli/actions/workflows/release.yml.
   Each crate's publish step skips if the version is already on
   crates.io, so re-running after a partial success is safe.
5. After the workflow completes, verify the leaf crate on crates.io
   (it depends on all the others, so its presence implies theirs):
   - https://crates.io/crates/harmont-cli/1.2.3

### Tagging in the monorepo (optional)

If you also want to mark the release in the monorepo for reference,
tag the same SHA the mirror sync was based on:

```sh
git -C <monorepo> tag cli-v1.2.3 <sha>
git -C <monorepo> push origin cli-v1.2.3
```

The monorepo tag has no automation behind it; the mirror tag is what
the publish workflow consumes.
