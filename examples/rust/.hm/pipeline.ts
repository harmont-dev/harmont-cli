import { pipeline, push, type PipelineDefinition } from "@harmont/hm";
import { rust } from "@harmont/hm/toolchains";

// project() warms a shared dependency cache so test/clippy/fmt reuse one compile.
const project = rust.project({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    // ci({ nextest: true }) → cargo nextest run + doctest + clippy + fmt.
    pipeline: pipeline(project.ci({ nextest: true }), {
      env: { CI: "true", RUST_BACKTRACE: "1" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
