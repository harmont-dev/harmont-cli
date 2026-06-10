import { pipeline, push, pullRequest, type PipelineDefinition } from "@harmont/hm";
import { cmake } from "@harmont/hm/toolchains";

const project = cmake({
  path: ".",
  compiler: "clang-18",
  defines: {
    CMAKE_BUILD_TYPE: "Release",
    CMAKE_CXX_STANDARD: "20",
    BUILD_TESTING: "ON",
  },
});

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" }), pullRequest()],
    pipeline: pipeline([project.test(), project.lint(), project.fmt()], {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
