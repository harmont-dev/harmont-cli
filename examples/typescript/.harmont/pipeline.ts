import { pipeline, push, type PipelineDefinition } from "harmont";
import { npm } from "harmont/toolchains";

const project = npm({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(project.run("build"), project.run("test"), project.run("lint"), {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
