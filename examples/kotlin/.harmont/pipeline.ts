import { pipeline, push, type PipelineDefinition } from "harmont";
import { gradle } from "harmont/toolchains";

const project = gradle({ path: ".", jdk: "21", kotlin: true });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(project.build(), project.test(), project.lint(), {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
