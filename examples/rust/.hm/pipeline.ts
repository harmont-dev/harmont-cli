import { pipeline, push, type PipelineDefinition } from "@harmont/hm";
import { rust } from "@harmont/hm/toolchains";

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
