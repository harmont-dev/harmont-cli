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

    if harmont_dir.exists() && force {
        std::fs::remove_dir_all(&harmont_dir)
            .with_context(|| format!("removing {}", harmont_dir.display()))?;
    }
    std::fs::create_dir_all(&harmont_dir)
        .with_context(|| format!("creating {}", harmont_dir.display()))?;
    let dest = harmont_dir.join(tmpl.filename);
    std::fs::write(&dest, tmpl.content).with_context(|| format!("writing {}", dest.display()))?;
    ensure_gitignore_entry(&harmont_dir, "node_modules/")?;
    ensure_gitignore_entry(&harmont_dir, "__pycache__/")?;
    Ok(true)
}

fn write_skills(dir: &Path) -> Result<()> {
    let skills: &[(&str, &str)] = &[
        ("validate-ci", SKILL_VALIDATE_CI),
        ("write-pipeline", SKILL_WRITE_PIPELINE),
        ("convert-gha", SKILL_CONVERT_GHA),
    ];
    for (slug, content) in skills {
        let skill_dir = dir.join(format!(".claude/skills/{slug}"));
        std::fs::create_dir_all(&skill_dir)
            .with_context(|| format!("creating {}", skill_dir.display()))?;
        let dest = skill_dir.join("SKILL.md");
        std::fs::write(&dest, content)
            .with_context(|| format!("writing {}", dest.display()))?;
        tracing::info!("installed Claude Code skill: .claude/skills/{slug}/SKILL.md");
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
        && std::fs::read_dir(&workflows)
            .map(|entries| {
                entries.filter_map(Result::ok).any(|e| {
                    let p = e.path();
                    matches!(
                        p.extension().and_then(|x| x.to_str()),
                        Some("yml" | "yaml")
                    )
                })
            })
            .unwrap_or(false)
}

/// # Errors
///
/// Returns an error if the target directory is unwritable, or if no template
/// can be determined in a non-interactive context.
#[allow(clippy::unused_async)]
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
        let kind = match args.template {
            Some(k) => k,
            None => {
                if !tty {
                    bail!(
                        "no template specified and no terminal available\n  \
                         hint: pass --template <name> in non-interactive contexts"
                    );
                }
                pick_interactive()?
            }
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

    if has_github_workflows(&args.dir) {
        tracing::info!(
            "detected GitHub Actions workflows in .github/workflows/\n  \
             hint: use the `convert-gha` Claude Code skill to migrate them to Harmont"
        );
    }

    // Skills are offered whenever a terminal is present, independent of
    // whether a template flag was passed.
    if tty && prompt_skills()? {
        write_skills(&args.dir)?;
    }

    tracing::info!("next step: run `hm run` to execute your pipeline locally");
    Ok(())
}
