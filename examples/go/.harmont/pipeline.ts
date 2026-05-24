import {
  pipeline,
  sh,
  target,
  push,
  forever,
  ttl,
  type PipelineDefinition,
} from "harmont";

const goInstalled = target("go-installed", () =>
  sh("apt-get update && apt-get install -y curl ca-certificates", {
    label: ":go: apt-base",
    cache: ttl(86400),
  }).sh(
    "curl -fsSL https://go.dev/dl/go1.22.4.linux-amd64.tar.gz | tar -C /usr/local -xzf -",
    { label: ":go: install", cache: forever() },
  ),
);

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    ir: pipeline(
      goInstalled().sh("go build ./...", { label: ":go: build" }),
      goInstalled().sh("go test ./...", { label: ":go: test" }),
      goInstalled().sh("go vet ./...", { label: ":go: vet" }),
      {
        env: { CI: "true" },
        defaultImage: "ubuntu:24.04",
      },
    ),
  },
];

export default pipelines;
