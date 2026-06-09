import { pipeline, push, type PipelineDefinition } from "harmont";
import { rust } from "harmont/toolchains";

const project = rust.toolchain({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(
      [project.build(), project.test(), project.clippy(), project.fmt()],
      { env: { CI: "true" }, defaultImage: "ubuntu:24.04" },
    ),
  },
];

export default pipelines;
