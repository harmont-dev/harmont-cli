import { pipeline, push, type PipelineDefinition } from "harmont";
import { cmake } from "harmont/toolchains";

const project = cmake({ path: ".", lang: "c" });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline([project.build(), project.test(), project.fmt()], {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
