use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Context, Result, bail};
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
                filename: "pipeline.ts",
                content: include_str!("init_templates/nextjs.ts"),
            },
            Self::Js => Template {
                label: "JavaScript / TypeScript",
                filename: "pipeline.ts",
                content: include_str!("init_templates/js.ts"),
            },
            Self::Rust => Template {
                label: "Rust",
                filename: "pipeline.py",
                content: include_str!("init_templates/rust.py"),
            },
            Self::Zig => Template {
                label: "Zig",
                filename: "pipeline.ts",
                content: include_str!("init_templates/zig.ts"),
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
async fn prompt_cloud_registration(dir: &std::path::Path) -> Result<()> {
    let cfg = hm_config::Config::load(None).unwrap_or_default();
    let api_url = &cfg.cloud.api_url;
    let is_logged_in = hm_config::creds::cloud_token(api_url).is_some();

    if !is_logged_in {
        let want_login = dialoguer::Confirm::new()
            .with_prompt("You are not logged in to Harmont Cloud. Log in now?")
            .default(false)
            .interact()
            .unwrap_or(false);

        if !want_login {
            return Ok(());
        }

        hm_plugin_cloud::login_interactive().await?;
    }

    let (client, _ctx) = hm_plugin_cloud::settings::client()
        .context("could not build authenticated cloud client")?;

    let orgs = client
        .raw()
        .list_organizations(None, None)
        .await
        .map_err(hm_plugin_cloud::settings::map_raw)
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

fn write_template(dir: &Path, tmpl: &Template, force: bool) -> Result<bool> {
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
    std::fs::create_dir_all(&harmont_dir)
        .with_context(|| format!("creating {}", harmont_dir.display()))?;
    let dest = harmont_dir.join(tmpl.filename);
    std::fs::write(&dest, tmpl.content).with_context(|| format!("writing {}", dest.display()))?;
    ensure_gitignore_entry(&harmont_dir, "node_modules/")?;
    ensure_gitignore_entry(&harmont_dir, "__pycache__/")?;
    Ok(true)
}

fn write_skills(dir: &Path, force: bool) -> Result<()> {
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
        std::fs::create_dir_all(&skill_dir)
            .with_context(|| format!("creating {}", skill_dir.display()))?;
        std::fs::write(&dest, content).with_context(|| format!("writing {}", dest.display()))?;
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
pub async fn handle(args: InitArgs) -> Result<()> {
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
        let wrote_pipeline = write_template(&args.dir, &tmpl, args.force)?;
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

    if tty && let Err(e) = prompt_cloud_registration(&args.dir).await {
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
        write_skills(&args.dir, args.force)?;
    }

    let project_config = hm_config::Config::project_config_path(&args.dir);
    if project_config.exists() {
        let cfg =
            hm_config::Config::load_from_paths(None, Some(&project_config)).unwrap_or_default();
        if cfg.backend == "cloud" {
            tracing::info!("next step: run `hm run` to execute your pipeline on Harmont Cloud");
        } else {
            tracing::info!("next step: run `hm run` to execute your pipeline locally");
        }
    } else {
        tracing::info!("next step: run `hm run` to execute your pipeline locally");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn skill_path(dir: &Path, slug: &str) -> std::path::PathBuf {
        dir.join(format!(".claude/skills/{slug}/SKILL.md"))
    }

    #[test]
    fn write_skills_installs_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        write_skills(dir.path(), false).unwrap();

        let dest = skill_path(dir.path(), "validate-ci");
        assert!(dest.exists());
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), SKILL_VALIDATE_CI);
    }

    #[test]
    fn write_skills_preserves_customized_file_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let dest = skill_path(dir.path(), "validate-ci");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, "# my local edits").unwrap();

        write_skills(dir.path(), false).unwrap();

        assert_eq!(
            std::fs::read_to_string(&dest).unwrap(),
            "# my local edits",
            "a customized skill must not be clobbered without --force"
        );
        // Other skills, which were absent, are still installed.
        assert!(skill_path(dir.path(), "write-pipeline").exists());
    }

    #[test]
    fn write_skills_force_overwrites_customized_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = skill_path(dir.path(), "validate-ci");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, "# my local edits").unwrap();

        write_skills(dir.path(), true).unwrap();

        assert_eq!(
            std::fs::read_to_string(&dest).unwrap(),
            SKILL_VALIDATE_CI,
            "--force must overwrite a customized skill with the bundled version"
        );
    }

    #[test]
    fn write_skills_skips_unchanged_file_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let dest = skill_path(dir.path(), "validate-ci");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, SKILL_VALIDATE_CI).unwrap();

        // Re-running with an identical, bundled file is a silent no-op.
        write_skills(dir.path(), false).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), SKILL_VALIDATE_CI);
    }
}
