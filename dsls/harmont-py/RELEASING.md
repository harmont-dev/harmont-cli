# Releasing harmont-py

This package lives upstream at <https://github.com/harmont-dev/harmont-py> and is mirrored into the private Harmont monorepo as `cidsl/py/`. The monorepo is the source of truth; the public repo receives a `git subtree push`.

## One-time setup

```sh
gh repo create harmont-dev/harmont-py --public \
  --description "Python DSL for Harmont CI pipelines" \
  --homepage https://harmont.dev

# Initial split from the monorepo (run from the monorepo root)
git subtree split --prefix=cidsl/py -b harmont-py-export
git push git@github.com:harmont-dev/harmont-py.git harmont-py-export:main
git branch -D harmont-py-export
```

## Ongoing sync (monorepo → public)

```sh
git subtree push --prefix=cidsl/py git@github.com:harmont-dev/harmont-py.git main
```

If subtree-push fails because the public repo has commits that aren't in the monorepo's history, fall back to a fresh split:

```sh
git subtree split --prefix=cidsl/py -b harmont-py-export
git push git@github.com:harmont-dev/harmont-py.git harmont-py-export:main
git branch -D harmont-py-export
```

## Pulling external contributions back (public → monorepo)

```sh
git subtree pull --prefix=cidsl/py git@github.com:harmont-dev/harmont-py.git main --squash
```

## Cutting a release

Versioning is **driven by git tags on the public mirror**. The release
workflow in `.github/workflows/release.yml` triggers on any tag matching
`v*`, seds the version from the tag into `pyproject.toml`, builds the
sdist and wheel, and publishes to PyPI via Trusted Publishing (OIDC —
no API tokens stored in the repo).

### Prerequisites (one-time)

1. **Configure the PyPI Trusted Publisher** on
   <https://pypi.org/manage/project/harmont/settings/publishing/> with:
   - Owner: `harmont-dev`
   - Repository: `harmont-py`
   - Workflow filename: `release.yml`
   - Environment: `release`

   If the `harmont` project does not yet exist on PyPI, create it via a
   one-off manual `twine upload` first (or use the "Add a pending
   publisher" flow at <https://pypi.org/manage/account/publishing/>),
   then add the Trusted Publisher.

2. **Create the `release` GitHub Environment** on
   <https://github.com/harmont-dev/harmont-py/settings/environments>.
   Recommended protection rules:
   - Deployment branches and tags → "Selected branches and tags" →
     add tag rule `v*`.
   - (Optional) required reviewers on the environment so a human has
     to click "approve" before publish runs.

### Releasing

1. Update `CHANGELOG.md` or release notes locally if you keep them.
2. Tag from the monorepo (source of truth):

   ```sh
   git tag v<version>
   git subtree push --prefix=cidsl/py git@github.com:harmont-dev/harmont-py.git main
   git push git@github.com:harmont-dev/harmont-py.git v<version>
   ```

   The tag has to land on the **public** repo for the workflow to fire.
   The subtree-push lands the corresponding `main` commit there first
   so the tag points at the right SHA.

3. Watch the run:

   ```sh
   gh run watch \
     "$(gh run list --repo harmont-dev/harmont-py --workflow release.yml \
        --limit 1 --json databaseId --jq '.[0].databaseId')" \
     --repo harmont-dev/harmont-py --exit-status
   ```

4. Confirm the release on <https://pypi.org/project/harmont/>.
5. (Optional) Create a GitHub Release on the same tag with notes:

   ```sh
   gh release create v<version> --repo harmont-dev/harmont-py \
     --title "harmont v<version>" --generate-notes
   ```

### Troubleshooting

- **`Trusted publishing exchange failed`:** the GH Environment name in
  the workflow does not match the one configured on PyPI. Both must be
  exactly `release`.
- **`File already exists`:** the version was already published to PyPI.
  PyPI is append-only — bump the version, re-tag, re-push.
- **`No files to upload`:** the build step did not produce
  `dist/*.tar.gz` and `dist/*.whl`. Inspect the `Build sdist and wheel`
  step output. Most common cause: `setuptools` couldn't find a package
  to build because `pyproject.toml` was mid-edit.
