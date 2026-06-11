import { pipeline, push, type PipelineDefinition } from "@harmont/hm";
import { rust } from "@harmont/hm/toolchains";

// project() warms a shared dependency cache so test/clippy/fmt reuse one compile.
const project = rust.project({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    // ci() → test + clippy + fmt, sharing one warmup. Add { nextest: true }
    // when cargo-nextest is available to also split out doctests.
    pipeline: pipeline(project.ci(), {
      env: { CI: "true", RUST_BACKTRACE: "1" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
