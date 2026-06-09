import { pipeline, push, type PipelineDefinition } from "@harmont/hm";
import { python } from "@harmont/hm/toolchains";

const project = python({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(
      [project.test(), project.lint(), project.fmt(), project.typecheck()],
      { env: { CI: "true" }, defaultImage: "ubuntu:24.04" },
    ),
  },
];

export default pipelines;
