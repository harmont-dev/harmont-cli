import { pipeline, push, type PipelineDefinition } from "harmont";
import { cmake } from "harmont/toolchains";

const project = cmake({ path: ".", buildType: "Release" });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(project.test(), project.fmt(), {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
