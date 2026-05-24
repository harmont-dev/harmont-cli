import { pipeline, push, type PipelineDefinition } from "harmont";
import { dotnet } from "harmont/toolchains";

const project = dotnet({ path: ".", channel: "8.0" });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(project.build(), project.test(), project.fmt(), {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
