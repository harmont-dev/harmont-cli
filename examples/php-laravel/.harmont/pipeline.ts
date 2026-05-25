import { pipeline, push, type PipelineDefinition } from "harmont";
import { composer } from "harmont/toolchains";

const project = composer({ path: ".", laravel: false });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(project.test(), project.lint(), {
      env: { CI: "true", APP_ENV: "testing" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
