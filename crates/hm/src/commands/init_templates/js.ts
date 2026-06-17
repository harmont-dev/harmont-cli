import { pipeline, push, type PipelineDefinition } from "@harmont/hm";
import { js } from "@harmont/hm/toolchains";

const project = js.project({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(
      [
        project.run("build"),
        project.run("test"),
        project.run("lint"),
        // Reference a secret stored on your org or pipeline (set it with `hm secret set`).
        // Import `secrets` from "@harmont/hm" to use this:
        // project.install().sh("deploy", { env: { DEPLOY_TOKEN: secrets["DEPLOY_TOKEN"] } }),
      ],
      {
        env: { CI: "true" },
      },
    ),
  },
];

export default pipelines;
