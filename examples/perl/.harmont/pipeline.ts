import { pipeline, push, type PipelineDefinition } from "harmont";
import { perl } from "harmont/toolchains";

const project = perl({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(project.test(), project.lint(), {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
