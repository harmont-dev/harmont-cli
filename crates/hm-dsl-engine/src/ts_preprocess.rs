//! TypeScript-to-JavaScript preprocessing.
//!
//! Two-stage pipeline:
//! 1. **Strip types** via `oxc_transformer` — parse TS, transform to JS, codegen.
//! 2. **Rewrite imports** — line-by-line text transform that rewrites
//!    `import { X } from 'harmont'` → `const { X } = globalThis.harmont;`
//!    and strips `import type ...` lines.

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{TransformOptions, Transformer, TypeScriptOptions};

/// Preprocess TypeScript source into plain JavaScript suitable for QuickJS.
///
/// Strips all TypeScript type syntax and rewrites `harmont` imports to
/// `globalThis.harmont` destructuring.
///
/// Import rewriting happens first (text level) so the transformer never sees
/// ESM `import` declarations for `harmont` modules.
pub fn preprocess_ts(source: &str) -> anyhow::Result<String> {
    let rewritten = rewrite_imports(source);
    let js = strip_types(&rewritten)?;
    Ok(js)
}

/// Parse TypeScript, strip type annotations via oxc, and codegen back to JS.
fn strip_types(source: &str) -> anyhow::Result<String> {
    let allocator = Allocator::default();
    let source_type = SourceType::default().with_typescript(true).with_module(true);

    // 1. Parse
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.panicked {
        anyhow::bail!("oxc parser panicked on TypeScript input");
    }

    // 2. Semantic analysis (required for Scoping used by transformer and codegen)
    //    Build semantic, extract scoping, then drop the borrow on `program`.
    let scoping = {
        let sem_ret = SemanticBuilder::new().build(&parsed.program);
        sem_ret.semantic.into_scoping()
    };

    // 3. Transform (strip TS types only — preserve value imports)
    let mut program = parsed.program;
    let options = TransformOptions {
        typescript: TypeScriptOptions {
            only_remove_type_imports: true,
            ..TypeScriptOptions::default()
        },
        ..TransformOptions::default()
    };

    let transformer = Transformer::new(&allocator, Path::new("pipeline.ts"), &options);
    let ret = transformer.build_with_scoping(scoping, &mut program);

    if !ret.errors.is_empty() {
        let msgs: Vec<String> = ret.errors.iter().map(ToString::to_string).collect();
        anyhow::bail!("oxc transform errors: {}", msgs.join("; "));
    }

    // 4. Codegen
    let codegen_ret = Codegen::new().build(&program);
    Ok(codegen_ret.code)
}

/// Rewrite `harmont` imports to `globalThis.harmont` destructuring.
///
/// - `import { X } from 'harmont';`       → `const { X } = globalThis.harmont;`
/// - `import { X } from 'harmont/foo';`   → `const { X } = globalThis.harmont.foo;`
/// - `import type { ... } from 'harmont';` → removed (oxc may have already done this)
/// - Other imports are left unchanged.
fn rewrite_imports(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim();

        // Skip type-only imports (should already be gone after strip_types,
        // but handle it defensively).
        if trimmed.starts_with("import type ") {
            continue;
        }

        // Match: import { ... } from 'harmont...'  or  import { ... } from "harmont..."
        if let Some(rewritten) = try_rewrite_harmont_import(trimmed) {
            output.push_str(&rewritten);
            output.push('\n');
            continue;
        }

        output.push_str(line);
        output.push('\n');
    }
    output
}

/// Try to rewrite a single `import { ... } from 'harmont...'` line.
/// Returns `None` if this is not a harmont import.
fn try_rewrite_harmont_import(line: &str) -> Option<String> {
    // Must start with `import`
    if !line.starts_with("import ") {
        return None;
    }

    // Extract the binding part between { and }
    let open = line.find('{')?;
    let close = line.find('}')?;
    let bindings = &line[open..=close]; // e.g. "{ sh, pipeline }"

    // Find the module specifier — look for `from` followed by a string literal
    let from_idx = line.find(" from ")?;
    let after_from = &line[from_idx + 6..]; // after "from "
    let after_from = after_from.trim();

    // Extract the module path from the string literal
    let (quote_char, rest) = if after_from.starts_with('\'') {
        ('\'', &after_from[1..])
    } else if after_from.starts_with('"') {
        ('"', &after_from[1..])
    } else {
        return None;
    };

    let end_quote = rest.find(quote_char)?;
    let module_path = &rest[..end_quote];

    // Only rewrite harmont imports
    if module_path != "harmont" && !module_path.starts_with("harmont/") {
        return None;
    }

    // Build the globalThis expression
    // 'harmont'         -> globalThis.harmont
    // 'harmont/foo'     -> globalThis.harmont.foo
    // 'harmont/foo/bar' -> globalThis.harmont.foo.bar
    let global_path = module_path.replace('/', ".");
    let global_expr = format!("globalThis.{global_path}");

    Some(format!("const {bindings} = {global_expr};"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_type_annotation() {
        let input = "const x: string = 'hi';\n";
        let output = preprocess_ts(input).unwrap();
        assert!(!output.contains(": string"), "output: {output}");
        assert!(output.contains("hi"), "output: {output}");
    }

    #[test]
    fn rewrites_harmont_import() {
        let input = "import { sh, pipeline } from 'harmont';\n";
        let output = preprocess_ts(input).unwrap();
        assert!(output.contains("globalThis.harmont"), "output: {output}");
        assert!(!output.contains("import"));
    }

    #[test]
    fn removes_type_only_import() {
        let input = "import type { Step } from 'harmont';\nconst x = 1;\n";
        let output = preprocess_ts(input).unwrap();
        assert!(!output.contains("import type"), "output: {output}");
        assert!(output.contains("1"));
    }

    #[test]
    fn rewrites_subpath_import() {
        let input = "import { npm } from 'harmont/toolchains';\n";
        let output = preprocess_ts(input).unwrap();
        assert!(
            output.contains("globalThis.harmont.toolchains"),
            "output: {output}"
        );
    }

    #[test]
    fn leaves_non_harmont_imports_alone() {
        let input = "import { foo } from './local';\n";
        let output = preprocess_ts(input).unwrap();
        assert!(output.contains("./local"), "output: {output}");
    }

    #[test]
    fn full_pipeline_file() {
        let input = r#"import { sh, pipeline } from 'harmont';
import type { Step } from 'harmont';

const base: Step = sh('echo hi');

export default [
  { slug: 'ci', pipeline: pipeline(base) }
];
"#;
        let output = preprocess_ts(input).unwrap();
        assert!(!output.contains("import type"), "type import present");
        assert!(output.contains("globalThis.harmont"), "missing destructure");
        assert!(!output.contains(": Step"), "type annotation present");
    }
}
