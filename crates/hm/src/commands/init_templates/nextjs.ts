import { pipeline, push, type PipelineDefinition } from "@harmont/hm";
import { js } from "@harmont/hm/toolchains";

const project = js.project({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline([project.run("build"), project.run("lint")], {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
