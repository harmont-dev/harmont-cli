use anyhow::{Context, Result, bail};

use crate::cli::init::InitArgs;

#[derive(Debug)]
struct Template {
    slug: &'static str,
    label: &'static str,
    filename: &'static str,
    content: &'static str,
}

const TEMPLATES: &[Template] = &[
    Template {
        slug: "cmake",
        label: "CMake",
        filename: "pipeline.py",
        content: "\"\"\"CMake CI pipeline.\"\"\"\nfrom __future__ import annotations\n\nimport harmont as hm\n\n\n@hm.pipeline(\n    \"ci\",\n    env={\"CI\": \"true\"},\n    default_image=\"ubuntu:24.04\",\n    triggers=[hm.push(branch=\"main\")],\n)\ndef ci() -> tuple[hm.Step, ...]:\n    project = hm.cmake(path=\".\")\n    return (\n        project.test(),\n        project.lint(),\n        project.fmt(),\n    )\n",
    },
    Template {
        slug: "elixir",
        label: "Elixir",
        filename: "pipeline.py",
        content: "\"\"\"Elixir CI pipeline.\"\"\"\nfrom __future__ import annotations\n\nimport harmont as hm\n\n\n@hm.pipeline(\n    \"ci\",\n    env={\"CI\": \"true\", \"MIX_ENV\": \"test\"},\n    default_image=\"ubuntu:24.04\",\n    triggers=[hm.push(branch=\"main\")],\n)\ndef ci() -> tuple[hm.Step, ...]:\n    project = hm.elixir(path=\".\")\n    return (\n        project.compile(),\n        project.test(),\n        project.format(),\n    )\n",
    },
    Template {
        slug: "nextjs",
        label: "Next.js",
        filename: "pipeline.ts",
        content: "import { pipeline, push, type PipelineDefinition } from \"harmont\";\nimport { js } from \"harmont/toolchains\";\n\nconst project = js.project({ path: \".\" });\n\nconst pipelines: PipelineDefinition[] = [\n  {\n    slug: \"ci\",\n    triggers: [push({ branch: \"main\" })],\n    pipeline: pipeline([project.run(\"build\"), project.run(\"test\"), project.run(\"lint\")], {\n      env: { CI: \"true\" },\n      defaultImage: \"ubuntu:24.04\",\n    }),\n  },\n];\n\nexport default pipelines;\n",
    },
    Template {
        slug: "js",
        label: "JavaScript / TypeScript",
        filename: "pipeline.ts",
        content: "import { pipeline, push, type PipelineDefinition } from \"harmont\";\nimport { js } from \"harmont/toolchains\";\n\nconst project = js.project({ path: \".\" });\n\nconst pipelines: PipelineDefinition[] = [\n  {\n    slug: \"ci\",\n    triggers: [push({ branch: \"main\" })],\n    pipeline: pipeline([project.run(\"build\"), project.run(\"test\"), project.run(\"lint\")], {\n      env: { CI: \"true\" },\n      defaultImage: \"ubuntu:24.04\",\n    }),\n  },\n];\n\nexport default pipelines;\n",
    },
    Template {
        slug: "rust",
        label: "Rust",
        filename: "pipeline.py",
        content: "\"\"\"Rust CI pipeline.\"\"\"\nfrom __future__ import annotations\n\nimport harmont as hm\nfrom harmont.rust import RustToolchain\n\n\n@hm.target()\ndef project() -> RustToolchain:\n    return hm.rust.toolchain(path=\".\")\n\n\n@hm.pipeline(\n    \"ci\",\n    env={\"CI\": \"true\"},\n    default_image=\"ubuntu:24.04\",\n    triggers=[hm.push(branch=\"main\")],\n)\ndef ci(project: hm.Target[RustToolchain]) -> tuple[hm.Step, ...]:\n    return (\n        project.build(),\n        project.test(),\n        project.clippy(),\n        project.fmt(),\n    )\n",
    },
    Template {
        slug: "zig",
        label: "Zig",
        filename: "pipeline.ts",
        content: "import { pipeline, push, type PipelineDefinition } from \"harmont\";\nimport { zig } from \"harmont/toolchains\";\n\nconst project = zig({ path: \".\" });\n\nconst pipelines: PipelineDefinition[] = [\n  {\n    slug: \"ci\",\n    triggers: [push({ branch: \"main\" })],\n    pipeline: pipeline([project.build(), project.test(), project.fmt()], {\n      env: { CI: \"true\" },\n      defaultImage: \"ubuntu:24.04\",\n    }),\n  },\n];\n\nexport default pipelines;\n",
    },
    Template {
        slug: "python",
        label: "Python",
        filename: "pipeline.py",
        content: "\"\"\"Python CI pipeline.\"\"\"\nfrom __future__ import annotations\n\nimport harmont as hm\nfrom harmont.python import PythonToolchain\n\n\n@hm.target()\ndef project() -> PythonToolchain:\n    return hm.python(path=\".\")\n\n\n@hm.pipeline(\n    \"ci\",\n    env={\"CI\": \"true\"},\n    default_image=\"ubuntu:24.04\",\n    triggers=[hm.push(branch=\"main\")],\n)\ndef ci(project: hm.Target[PythonToolchain]) -> tuple[hm.Step, ...]:\n    return (\n        project.test(),\n        project.lint(),\n        project.fmt(),\n        project.typecheck(),\n    )\n",
    },
];

fn find_template(slug: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.slug == slug)
}

fn pick_template_interactive() -> Result<&'static Template> {
    let labels: Vec<&str> = TEMPLATES.iter().map(|t| t.label).collect();
    let selection = dialoguer::Select::new()
        .with_prompt("Select a project template")
        .items(&labels)
        .default(0)
        .interact()
        .context("template selection cancelled")?;
    Ok(&TEMPLATES[selection])
}

fn write_template(dir: &std::path::Path, template: &Template, force: bool) -> Result<()> {
    let harmont_dir = dir.join(".harmont");
    if harmont_dir.exists() && !force {
        bail!(
            ".harmont/ already exists in {}\n  \
             hint: use --force to overwrite",
            dir.display()
        );
    }
    std::fs::create_dir_all(&harmont_dir)
        .with_context(|| format!("creating {}", harmont_dir.display()))?;
    let dest = harmont_dir.join(template.filename);
    std::fs::write(&dest, template.content)
        .with_context(|| format!("writing {}", dest.display()))?;
    Ok(())
}

/// Scaffold a `.harmont/` pipeline directory from a project template.
///
/// # Errors
///
/// Returns an error if the template is unknown, the target directory cannot be
/// determined, or the filesystem write fails.
#[allow(clippy::unused_async)]
pub async fn handle(args: InitArgs) -> Result<()> {
    let dir = match args.dir {
        Some(d) => d,
        None => std::env::current_dir().context("cannot determine current directory")?,
    };

    let template = match &args.template {
        Some(slug) => find_template(slug).ok_or_else(|| {
            let available: Vec<&str> = TEMPLATES.iter().map(|t| t.slug).collect();
            anyhow::anyhow!(
                "unknown template {:?}\n  available: {}",
                slug,
                available.join(", ")
            )
        })?,
        None => pick_template_interactive()?,
    };

    write_template(&dir, template, args.force)?;

    let ext = if std::path::Path::new(template.filename)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ts"))
    {
        "TypeScript"
    } else {
        "Python"
    };
    tracing::info!(
        "created .harmont/{} ({ext} pipeline, template: {})",
        template.filename,
        template.slug
    );
    tracing::info!("next step: run `hm run` to execute your pipeline locally");
    Ok(())
}
