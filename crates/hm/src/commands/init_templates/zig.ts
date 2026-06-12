import { pipeline, push, type PipelineDefinition } from "@harmont/hm";
import { zig } from "@harmont/hm/toolchains";

const project = zig({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline([project.build(), project.test(), project.fmt()], {
      env: { CI: "true" },
    }),
  },
];

export default pipelines;
