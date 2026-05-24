import { pipeline, push, pullRequest, type PipelineDefinition } from "harmont";
import { rust, py } from "harmont/toolchains";

const rustProject = rust({ path: "." });
const pyProject = py.uv({ path: "dsls/harmont-py" });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" }), pullRequest({ branches: ["main"] })],
    pipeline: pipeline(
      rustProject.build(),
      rustProject.install().sh(`. $HOME/.cargo/env && cd . && cargo test --lib`, { label: ":rust: test" }),
      rustProject.clippy(),
      rustProject.fmt(),
      pyProject.lint(),
      pyProject.fmt(),
      pyProject.typecheck({ paths: "harmont" }),
      pyProject.run(
        "pytest -v --deselect tests/test_gradle.py --deselect tests/test_haskell.py",
        { label: ":python: test" },
      ),
      { env: { CI: "true" }, defaultImage: "ubuntu:24.04" },
    ),
  },
];

export default pipelines;
