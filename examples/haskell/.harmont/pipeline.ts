import { pipeline, push, target, type PipelineDefinition } from "harmont";
import { haskell } from "harmont/toolchains";

const ghc = target("ghc", () => haskell({ ghc: "9.6.7" }));
const project = target("project", () => ghc().cabal("."));

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(
      project().build(),
      project().test(),
      project().lint(),
      project().fmt(),
      { env: { CI: "true" }, defaultImage: "ubuntu:24.04" },
    ),
  },
];

export default pipelines;
