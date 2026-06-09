import { pipeline, push, pullRequest, type PipelineDefinition } from "harmont";
import { elixir } from "harmont/toolchains";

const project = elixir({
  path: ".",
  elixirVersion: "1.18.3",
  otpVersion: "27.3.3",
});

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" }), pullRequest()],
    pipeline: pipeline(
      [
        project.compile(),
        project.test({ cover: true }),
        project.format(),
        project.credo(),
        project.dialyzer(),
        project.sobelow(),
        project.depsAudit(),
        project.hexAudit(),
      ],
      {
        env: {
          CI: "true",
          MIX_ENV: "test",
        },
        defaultImage: "ubuntu:24.04",
      },
    ),
  },
  {
    slug: "deploy",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(
      [project.compile(), project.mix("assets.deploy"), project.release()],
      { env: { MIX_ENV: "prod" }, defaultImage: "ubuntu:24.04" },
    ),
  },
];

export default pipelines;
