import { pipeline, push, scratch, target, ttl, type PipelineDefinition } from "harmont";
import { npm, zig } from "harmont/toolchains";

const aptBase = target("apt-base", () =>
  scratch({ image: "ubuntu:24.04" }).sh(
    "apt-get update && apt-get install -y --no-install-recommends curl ca-certificates xz-utils",
    { label: ":apt: base", cache: ttl(86400) },
  ),
);

const zigTc = target("zig", () => zig({ base: aptBase() }));
const zigLibA = target("zig-lib-a", () => zigTc().project("zig-a"));
const zigLibB = target("zig-lib-b", () => zigTc().project("zig-b"));
const webProject = target("web-project", () => npm({ path: "web", base: aptBase() }));

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(
      zigLibA().build(),
      zigLibA().test(),
      zigLibB().build(),
      zigLibB().test(),
      webProject().run("build"),
      webProject().run("test"),
      webProject().run("lint"),
      { env: { CI: "true" }, defaultImage: "ubuntu:24.04" },
    ),
  },
];

export default pipelines;
