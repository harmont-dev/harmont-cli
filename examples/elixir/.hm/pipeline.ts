import { pipeline, push, type PipelineDefinition } from "@harmont/hm";
import { elixir } from "@harmont/hm/toolchains";

const project = elixir({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(
      [
        project.compile(),
        project.test(),
        project.format(),
        project.credo(),
        project.dialyzer(),
        project.depsAudit(),
        project.hexAudit(),
      ],
      { env: { CI: "true", MIX_ENV: "test" }, defaultImage: "ubuntu:24.04" },
    ),
  },
];

export default pipelines;
