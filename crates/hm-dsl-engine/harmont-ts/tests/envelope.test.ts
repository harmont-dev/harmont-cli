import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { forever } from "../src/cache.js";
import { renderEnvelope, type PipelineDefinition } from "../src/envelope.js";
import { pipeline } from "../src/pipeline.js";
import { sh } from "../src/step.js";
import { push, pullRequest } from "../src/triggers.js";

function makeDef(overrides?: Partial<PipelineDefinition>): PipelineDefinition {
  return {
    slug: "ci",
    pipeline: pipeline([sh("echo", { label: "test" })]),
    ...overrides,
  };
}

describe("renderEnvelope", () => {
  it("produces schema_version 1 envelope", () => {
    const json = renderEnvelope([makeDef()]);
    const parsed = JSON.parse(json);
    expect(parsed.schema_version).toBe("1");
    expect(parsed.pipelines).toHaveLength(1);
  });

  it("includes slug, name, allow_manual, triggers, definition", () => {
    const json = renderEnvelope([
      makeDef({
        slug: "my-pipeline",
        name: "My Pipeline",
        allowManual: false,
        triggers: [push({ branch: "main" })],
      }),
    ]);
    const parsed = JSON.parse(json);
    const p = parsed.pipelines[0];
    expect(p.slug).toBe("my-pipeline");
    expect(p.name).toBe("My Pipeline");
    expect(p.allow_manual).toBe(false);
    expect(p.triggers).toEqual([{ event: "push", branches: ["main"] }]);
    expect(p.definition.version).toBe("0");
  });

  it("defaults name to slug, allowManual to true, triggers to empty", () => {
    const json = renderEnvelope([makeDef({ slug: "ci" })]);
    const parsed = JSON.parse(json);
    const p = parsed.pipelines[0];
    expect(p.name).toBe("ci");
    expect(p.allow_manual).toBe(true);
    expect(p.triggers).toEqual([]);
  });

  it("handles multiple pipelines", () => {
    const json = renderEnvelope([
      makeDef({ slug: "ci" }),
      makeDef({ slug: "deploy" }),
    ]);
    const parsed = JSON.parse(json);
    expect(parsed.pipelines).toHaveLength(2);
    expect(parsed.pipelines[0].slug).toBe("ci");
    expect(parsed.pipelines[1].slug).toBe("deploy");
  });

  it("resolves cache keys when basePath is provided", () => {
    const tmp = mkdtempSync(join(tmpdir(), "envelope-test-"));
    const def: PipelineDefinition = {
      slug: "ci",
      pipeline: pipeline([sh("apt-get update", { label: "apt", cache: forever() })]),
    };
    const json = renderEnvelope([def], { basePath: tmp, now: 1000000 });
    const parsed = JSON.parse(json);
    const cache = parsed.pipelines[0].definition.graph.nodes[0].step.cache;
    expect(cache.key).toBeTypeOf("string");
    expect(cache.key.length).toBe(64);
  });

  it("skips cache key resolution when basePath is absent", () => {
    const def: PipelineDefinition = {
      slug: "ci",
      pipeline: pipeline([sh("apt-get update", { label: "apt", cache: forever() })]),
    };
    const json = renderEnvelope([def]);
    const parsed = JSON.parse(json);
    const cache = parsed.pipelines[0].definition.graph.nodes[0].step.cache;
    expect(cache.key).toBeUndefined();
  });
});
