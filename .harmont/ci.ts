import {
  pipeline,
  push,
  pullRequest,
  aptBase,
  type PipelineDefinition,
} from "harmont";
import { rust, py } from "harmont/toolchains";

const base = aptBase({
  packages: [
    "curl",
    "ca-certificates",
    "build-essential",
    "pkg-config",
    "libssl-dev",
    "python3",
    "python3-venv",
  ],
});

const rustProject = rust.project({ path: ".", base });
const pyProject = py.uv({ path: "dsls/harmont-py", base });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" }), pullRequest({ branches: ["main"] })],
    pipeline: pipeline(
      rustProject.test({ flags: ["--lib"] }),
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
