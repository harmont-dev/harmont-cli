import { pipeline, push, type PipelineDefinition } from "@harmont/hm";
import { cmake } from "@harmont/hm/toolchains";

const project = cmake({ path: ".", defines: { CMAKE_BUILD_TYPE: "Release", CMAKE_CXX_STANDARD: "17" } });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline([project.test(), project.lint(), project.fmt()], {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
