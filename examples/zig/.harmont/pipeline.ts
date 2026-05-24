import { pipeline, push, type PipelineDefinition } from "harmont";
import { zig } from "harmont/toolchains";

const project = zig({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(project.build(), project.test(), project.fmt(), {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
