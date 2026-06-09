use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::cli::init::{InitArgs, TemplateKind};

struct Template {
    label: &'static str,
    filename: &'static str,
    content: &'static str,
    /// Optional companion file (e.g. package.json for TS templates).
    companion: Option<(&'static str, &'static str)>,
}

const HARMONT_PACKAGE_JSON: &str = r#"{
  "private": true,
  "type": "module",
  "devDependencies": {
    "harmont": "latest"
  }
}
"#;

impl TemplateKind {
    const fn meta(self) -> Template {
        match self {
            Self::Cmake => Template {
                label: "CMake",
                filename: "pipeline.py",
                content: include_str!("init_templates/cmake.py"),
                companion: None,
            },
            Self::Elixir => Template {
                label: "Elixir",
                filename: "pipeline.py",
                content: include_str!("init_templates/elixir.py"),
                companion: None,
            },
            Self::Nextjs => Template {
                label: "Next.js",
                filename: "pipeline.ts",
                content: include_str!("init_templates/nextjs.ts"),
                companion: Some(("package.json", HARMONT_PACKAGE_JSON)),
            },
            Self::Js => Template {
                label: "JavaScript / TypeScript",
                filename: "pipeline.ts",
                content: include_str!("init_templates/js.ts"),
                companion: Some(("package.json", HARMONT_PACKAGE_JSON)),
            },
            Self::Rust => Template {
                label: "Rust",
                filename: "pipeline.py",
                content: include_str!("init_templates/rust.py"),
                companion: None,
            },
            Self::Zig => Template {
                label: "Zig",
                filename: "pipeline.ts",
                content: include_str!("init_templates/zig.ts"),
                companion: Some(("package.json", HARMONT_PACKAGE_JSON)),
            },
            Self::Python => Template {
                label: "Python",
                filename: "pipeline.py",
                content: include_str!("init_templates/python.py"),
                companion: None,
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

fn write_template(dir: &Path, tmpl: &Template, force: bool) -> Result<()> {
    let harmont_dir = dir.join(".hm");
    if harmont_dir.exists() && !force {
        bail!(
            ".hm/ already exists in {}\n  \
             hint: use --force to overwrite",
            dir.display()
        );
    }
    if harmont_dir.exists() {
        std::fs::remove_dir_all(&harmont_dir)
            .with_context(|| format!("removing {}", harmont_dir.display()))?;
    }
    std::fs::create_dir_all(&harmont_dir)
        .with_context(|| format!("creating {}", harmont_dir.display()))?;
    let dest = harmont_dir.join(tmpl.filename);
    std::fs::write(&dest, tmpl.content)
        .with_context(|| format!("writing {}", dest.display()))?;
    if let Some((companion_name, companion_content)) = tmpl.companion {
        let companion_dest = harmont_dir.join(companion_name);
        std::fs::write(&companion_dest, companion_content)
            .with_context(|| format!("writing {}", companion_dest.display()))?;
    }
    ensure_gitignore_entry(&harmont_dir, "node_modules/")?;
    ensure_gitignore_entry(&harmont_dir, "__pycache__/")?;
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

/// # Errors
///
/// Returns an error if the target directory is unwritable or `.hm/`
/// already exists without `--force`.
#[allow(clippy::unused_async)]
pub async fn handle(args: InitArgs) -> Result<()> {
    let kind = match args.template {
        Some(k) => k,
        None => pick_interactive()?,
    };
    let tmpl = kind.meta();

    write_template(&args.dir, &tmpl, args.force)?;

    let dsl = match kind {
        TemplateKind::Nextjs | TemplateKind::Js | TemplateKind::Zig => "TypeScript",
        _ => "Python",
    };
    tracing::info!("created .hm/{} ({dsl} pipeline, template: {kind:?})", tmpl.filename);

    match dsl {
        "TypeScript" => {
            tracing::info!("for IDE type support, install the SDK:");
            tracing::info!("  cd .harmont && npm install");
        }
        "Python" => {
            tracing::info!("for IDE type support, install the SDK:");
            tracing::info!("  pip install harmont");
        }
        _ => {}
    }

    tracing::info!("next step: run `hm run` to execute your pipeline locally");
    Ok(())
}
