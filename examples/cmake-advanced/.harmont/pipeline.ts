import { pipeline, push, pullRequest, type PipelineDefinition } from "harmont";
import { cmake } from "harmont/toolchains";

const project = cmake({
  path: ".",
  compiler: "clang-18",
  buildType: "Release",
  std: 20,
  defines: { BUILD_TESTING: "ON" },
});

const sanProject = cmake({ path: ".", compiler: "clang-18" });
const covProject = cmake({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" }), pullRequest()],
    pipeline: pipeline(project.test(), project.lint(), project.fmt(), {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
  {
    slug: "sanitizers",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(sanProject.sanitize("asan"), sanProject.sanitize("tsan"), {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
  {
    slug: "coverage",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(covProject.coverage(), {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
