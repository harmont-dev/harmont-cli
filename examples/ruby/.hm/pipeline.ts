import { pipeline, push, type PipelineDefinition } from "@harmont/hm";
import { ruby } from "@harmont/hm/toolchains";

const project = ruby({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline([project.test(), project.lint()], {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
