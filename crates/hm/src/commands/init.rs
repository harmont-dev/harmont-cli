use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Context, Result, bail};
use hm_core::config::domain::BackendConfig;
use hm_dsl_engine::detect;

use crate::cli::init::{InitArgs, TemplateKind};

const SKILL_VALIDATE_CI: &str = include_str!("init_templates/skill_validate_ci.md");
const SKILL_WRITE_PIPELINE: &str = include_str!("init_templates/skill_write_pipeline.md");
const SKILL_CONVERT_GHA: &str = include_str!("init_templates/skill_convert_gha.md");

struct Template {
    label: &'static str,
    filename: &'static str,
    content: &'static str,
}

impl TemplateKind {
    const fn meta(self) -> Template {
        match self {
            Self::Cmake => Template {
                label: "CMake",
                filename: "pipeline.py",
                content: include_str!("init_templates/cmake.py"),
            },
            Self::Elixir => Template {
                label: "Elixir",
                filename: "pipeline.py",
                content: include_str!("init_templates/elixir.py"),
            },
            Self::Nextjs => Template {
                label: "Next.js",
                filename: "pipeline.py",
                content: include_str!("init_templates/nextjs.py"),
            },
            Self::Js => Template {
                label: "JavaScript / TypeScript",
                filename: "pipeline.py",
                content: include_str!("init_templates/js.py"),
            },
            Self::Rust => Template {
                label: "Rust",
                filename: "pipeline.py",
                content: include_str!("init_templates/rust.py"),
            },
            Self::Zig => Template {
                label: "Zig",
                filename: "pipeline.py",
                content: include_str!("init_templates/zig.py"),
            },
            Self::Python => Template {
                label: "Python",
                filename: "pipeline.py",
                content: include_str!("init_templates/python.py"),
            },
        }
    }
}

const ALL: &[TemplateKind] = &[
    TemplateKind::Cmake,
    TemplateKind::Elixir,
    TemplateKind::Nextjs,
    TemplateKind::Js,
    TemplateKind::Rust,
    TemplateKind::Zig,
    TemplateKind::Python,
];

fn pick_interactive() -> Result<TemplateKind> {
    let labels: Vec<&str> = ALL.iter().map(|k| k.meta().label).collect();
    let i = dialoguer::Select::new()
        .with_prompt("Select a project template")
        .items(&labels)
        .default(0)
        .interact()
        .context("template selection cancelled")?;
    Ok(ALL[i])
}

fn prompt_skills() -> Result<bool> {
    let install = dialoguer::Confirm::new()
        .with_prompt("Install Claude Code skills for hm?")
        .default(true)
        .interact()
        .context("skills prompt cancelled")?;
    Ok(install)
}

/// Prompt the user to link this repo to a Harmont Cloud organization.
///
/// Flow:
/// - If not logged in → offer to log in first (Confirm, default no).
/// - If logged in (or just logged in) → fetch orgs → Select with "No, skip" as first item.
/// - On org selection → write a sparse `.hm/config.toml` with `backend = "cloud"` and the org slug.
///
/// Silently returns `Ok(())` on any user-cancellation (Esc, Ctrl-C on a prompt).
async fn prompt_cloud_registration(
    dir: &std::path::Path,
    app: &hm_core::app_ctx::AppCtx,
) -> Result<()> {
    let is_logged_in = app.creds().get().await.is_some();

    if !is_logged_in {
        let want_login = dialoguer::Confirm::new()
            .with_prompt("You are not logged in to Harmont Cloud. Log in now?")
            .default(false)
            .interact()
            .unwrap_or(false);

        if !want_login {
            return Ok(());
        }

        crate::commands::cloud::login_interactive(app).await?;
    }

    let (client, _ctx) = crate::commands::cloud::settings::client(app)
        .await
        .context("could not build authenticated cloud client")?;

    let orgs = client
        .raw()
        .list_organizations(None, None)
        .await
        .map_err(crate::commands::cloud::settings::map_raw)
        .context("fetching organizations")?
        .into_inner();

    if orgs.data.is_empty() {
        tracing::warn!("no organizations found — create one at https://app.harmont.dev");
        return Ok(());
    }

    let mut items: Vec<String> = vec!["No, skip".to_string()];
    items.extend(orgs.data.iter().map(|o| format!("{} ({})", o.name, o.slug)));

    let selection = dialoguer::Select::new()
        .with_prompt("Link this repo to Harmont Cloud?")
        .items(&items)
        .default(0)
        .interact()
        .unwrap_or(0);

    if selection == 0 {
        return Ok(());
    }

    let chosen = &orgs.data[selection - 1];
    write_cloud_project_config(dir, &chosen.slug)?;
    tracing::info!(
        "linked to {} ({}) — `hm run` will now use Harmont Cloud by default",
        chosen.name,
        chosen.slug,
    );
    Ok(())
}

fn write_cloud_project_config(dir: &std::path::Path, org_slug: &str) -> Result<()> {
    let config_path = dir.join(".hm/config.toml");
    let content = format!(
        "backend = \"cloud\"\n\
         \n\
         [cloud]\n\
         org = \"{org_slug}\"\n"
    );
    std::fs::write(&config_path, &content)
        .with_context(|| format!("writing {}", config_path.display()))?;
    Ok(())
}

async fn write_template(dir: &Path, tmpl: &Template, force: bool) -> Result<bool> {
    let harmont_dir = dir.join(".hm");
    let already_has_pipeline = detect::has_pipeline_files(dir);

    if harmont_dir.exists() && already_has_pipeline && !force {
        tracing::warn!(
            "pipeline already exists in {}/.hm/ — skipping template\n  \
             hint: use --force to overwrite",
            dir.display()
        );
        return Ok(false);
    }

    // `--force` overwrites only the single target template file. We never
    // wipe the whole `.hm/` directory: that would also delete config.toml,
    // .gitignore, and any co-resident pipeline (e.g. a repo with both
    // pipeline.py and deploy.py). `std::fs::write` clobbers just the target.
    let dest = harmont_dir.join(tmpl.filename);
    hm_common::fs::write_create_all(&dest, tmpl.content)
        .await
        .with_context(|| format!("writing {}", dest.display()))?;
    ensure_gitignore_entry(&harmont_dir, "node_modules/")?;
    ensure_gitignore_entry(&harmont_dir, "__pycache__/")?;
    Ok(true)
}

async fn write_skills(dir: &Path, force: bool) -> Result<()> {
    let skills: &[(&str, &str)] = &[
        ("validate-ci", SKILL_VALIDATE_CI),
        ("write-pipeline", SKILL_WRITE_PIPELINE),
        ("convert-gha", SKILL_CONVERT_GHA),
    ];
    for (slug, content) in skills {
        let skill_dir = dir.join(format!(".claude/skills/{slug}"));
        let dest = skill_dir.join("SKILL.md");

        // Never silently clobber a customized skill. If the file is already
        // present and the user edited it, leave it alone unless --force is set.
        if dest.exists() && !force {
            let existing = std::fs::read_to_string(&dest)
                .with_context(|| format!("reading {}", dest.display()))?;
            if existing == *content {
                continue;
            }
            tracing::warn!(
                "skill .claude/skills/{slug}/SKILL.md already exists with local edits — skipping\n  \
                 hint: pass --force to overwrite it with the bundled version"
            );
            continue;
        }

        let updated = dest.exists();
        hm_common::fs::write_create_all(&dest, content)
            .await
            .with_context(|| format!("writing {}", dest.display()))?;
        if updated {
            tracing::info!("overwrote Claude Code skill: .claude/skills/{slug}/SKILL.md");
        } else {
            tracing::info!("installed Claude Code skill: .claude/skills/{slug}/SKILL.md");
        }
    }
    Ok(())
}

fn ensure_gitignore_entry(dir: &Path, entry: &str) -> Result<()> {
    let gitignore = dir.join(".gitignore");
    if gitignore.exists() {
        let content = std::fs::read_to_string(&gitignore)
            .with_context(|| format!("reading {}", gitignore.display()))?;
        if content.lines().any(|l| l.trim() == entry) {
            return Ok(());
        }
        let sep = if content.ends_with('\n') { "" } else { "\n" };
        std::fs::write(&gitignore, format!("{content}{sep}{entry}\n"))
            .with_context(|| format!("updating {}", gitignore.display()))?;
    } else {
        std::fs::write(&gitignore, format!("{entry}\n"))
            .with_context(|| format!("creating {}", gitignore.display()))?;
    }
    Ok(())
}

fn has_github_workflows(dir: &Path) -> bool {
    let workflows = dir.join(".github/workflows");
    workflows.is_dir()
        && std::fs::read_dir(&workflows).is_ok_and(|entries| {
            entries.filter_map(Result::ok).any(|e| {
                let p = e.path();
                matches!(p.extension().and_then(|x| x.to_str()), Some("yml" | "yaml"))
            })
        })
}

/// # Errors
///
/// Returns an error if the target directory is unwritable, or if no template
/// can be determined in a non-interactive context.
pub async fn handle(args: InitArgs, app: &hm_core::app_ctx::AppCtx) -> Result<()> {
    let tty = std::io::stdin().is_terminal();
    let has_pipeline = detect::has_pipeline_files(&args.dir);

    // Skip template selection entirely when a pipeline already exists and the
    // user didn't force an overwrite: they're re-running `hm init` to install
    // Claude skills, not to replace their pipeline.
    let skip_template = args.template.is_none() && has_pipeline && !args.force;

    if skip_template {
        tracing::info!("existing pipeline detected in .hm/ — skipping template selection");
    } else {
        let kind = if let Some(k) = args.template {
            k
        } else {
            if !tty {
                bail!(
                    "no template specified and no terminal available\n  \
                     hint: pass --template <name> in non-interactive contexts"
                );
            }
            pick_interactive()?
        };
        let tmpl = kind.meta();
        let wrote_pipeline = write_template(&args.dir, &tmpl, args.force).await?;
        if wrote_pipeline {
            let dsl = match kind {
                TemplateKind::Nextjs | TemplateKind::Js | TemplateKind::Zig => "TypeScript",
                _ => "Python",
            };
            tracing::info!(
                "created .hm/{} ({dsl} pipeline, template: {kind:?})",
                tmpl.filename
            );
        }
    }

    if tty && let Err(e) = prompt_cloud_registration(&args.dir, app).await {
        tracing::warn!("cloud registration skipped: {e:#}");
    }

    if has_github_workflows(&args.dir) {
        tracing::info!(
            "detected GitHub Actions workflows in .github/workflows/\n  \
             hint: use the `convert-gha` Claude Code skill to migrate them to Harmont"
        );
    }

    // Skills are offered whenever a terminal is present, independent of
    // whether a template flag was passed.
    if tty && prompt_skills()? {
        write_skills(&args.dir, args.force).await?;
    }

    let backend = hm_core::project_ctx::ProjectCtx::at(app, args.dir.clone())
        .await
        .map_or(BackendConfig::Docker, |p| p.config().backend.clone());
    match backend {
        BackendConfig::Cloud(_) => {
            tracing::info!("next step: run `hm run` to execute your pipeline on Harmont Cloud");
        }
        BackendConfig::Docker => {
            tracing::info!("next step: run `hm run` to execute your pipeline locally");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test setup and assertions")]

    use rstest::rstest;

    use super::*;

    fn skill_path(dir: &Path, slug: &str) -> std::path::PathBuf {
        dir.join(format!(".claude/skills/{slug}/SKILL.md"))
    }

    #[rstest]
    #[case::absent(None, false, SKILL_VALIDATE_CI)]
    #[case::customized_no_force(Some("# my local edits"), false, "# my local edits")]
    #[case::customized_force(Some("# my local edits"), true, SKILL_VALIDATE_CI)]
    #[case::unchanged_idempotent(Some(SKILL_VALIDATE_CI), false, SKILL_VALIDATE_CI)]
    #[tokio::test]
    async fn write_skills_behaves(
        #[case] preexisting: Option<&str>,
        #[case] force: bool,
        #[case] expected: &str,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let dest = skill_path(dir.path(), "validate-ci");
        if let Some(content) = preexisting {
            std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std::fs::write(&dest, content).unwrap();
        }

        write_skills(dir.path(), force).await.unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), expected);
    }

    #[rstest]
    #[tokio::test]
    async fn write_skills_installs_sibling_skills_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        write_skills(dir.path(), false).await.unwrap();

        // Skills other than the one under test are installed too.
        assert!(skill_path(dir.path(), "write-pipeline").exists());
    }
}
