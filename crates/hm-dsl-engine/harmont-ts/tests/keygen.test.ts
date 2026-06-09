import { createHash } from "node:crypto";
import { mkdtempSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { describe, expect, it, beforeEach } from "vitest";
import { resolvePipelineCacheKeys, type CacheKeyOptions } from "../src/keygen.js";
import { pipeline, type PipelineIR } from "../src/pipeline.js";
import { sh } from "../src/step.js";
import { forever, ttl, onChange } from "../src/cache.js";

function sha256(s: string): string {
  return createHash("sha256").update(s, "utf8").digest("hex");
}

const NUL = "\0";

function makeOpts(overrides?: Partial<CacheKeyOptions>): CacheKeyOptions {
  return {
    pipelineOrg: "test-org",
    pipelineSlug: "ci",
    now: 1000000,
    basePath: "/tmp/test",
    env: {},
    ...overrides,
  };
}

describe("resolvePipelineCacheKeys", () => {
  it("adds cache.key to forever-cached steps", () => {
    const ir = pipeline([sh("echo hello", { label: "greet", cache: forever() })]);
    const opts = makeOpts();

    resolvePipelineCacheKeys(ir.graph, opts);

    const cache = ir.graph.nodes[0].step.cache as Record<string, unknown>;
    expect(cache.key).toBeTypeOf("string");
    expect((cache.key as string).length).toBe(64);
  });

  it("produces deterministic keys", () => {
    const ir1 = pipeline([sh("echo hello", { label: "greet", cache: forever() })]);
    const ir2 = pipeline([sh("echo hello", { label: "greet", cache: forever() })]);
    const opts = makeOpts();

    resolvePipelineCacheKeys(ir1.graph, opts);
    resolvePipelineCacheKeys(ir2.graph, opts);

    const k1 = (ir1.graph.nodes[0].step.cache as Record<string, unknown>).key;
    const k2 = (ir2.graph.nodes[0].step.cache as Record<string, unknown>).key;
    expect(k1).toBe(k2);
  });

  it("different commands produce different keys", () => {
    const ir1 = pipeline([sh("echo a", { label: "a", cache: forever() })]);
    const ir2 = pipeline([sh("echo b", { label: "b", cache: forever() })]);
    const opts = makeOpts();

    resolvePipelineCacheKeys(ir1.graph, opts);
    resolvePipelineCacheKeys(ir2.graph, opts);

    const k1 = (ir1.graph.nodes[0].step.cache as Record<string, unknown>).key;
    const k2 = (ir2.graph.nodes[0].step.cache as Record<string, unknown>).key;
    expect(k1).not.toBe(k2);
  });

  it("different orgs produce different keys", () => {
    const ir1 = pipeline([sh("echo a", { label: "a", cache: forever() })]);
    const ir2 = pipeline([sh("echo a", { label: "a", cache: forever() })]);

    resolvePipelineCacheKeys(ir1.graph, makeOpts({ pipelineOrg: "org-a" }));
    resolvePipelineCacheKeys(ir2.graph, makeOpts({ pipelineOrg: "org-b" }));

    const k1 = (ir1.graph.nodes[0].step.cache as Record<string, unknown>).key;
    const k2 = (ir2.graph.nodes[0].step.cache as Record<string, unknown>).key;
    expect(k1).not.toBe(k2);
  });

  it("skips steps with no cache", () => {
    const ir = pipeline([sh("echo hello", { label: "greet" })]);
    resolvePipelineCacheKeys(ir.graph, makeOpts());
    expect(ir.graph.nodes[0].step.cache).toBeUndefined();
  });

  it("ttl bucket changes key", () => {
    const ir1 = pipeline([sh("apt-get", { label: "apt", cache: ttl(86400) })]);
    const ir2 = pipeline([sh("apt-get", { label: "apt", cache: ttl(86400) })]);

    resolvePipelineCacheKeys(ir1.graph, makeOpts({ now: 86400 * 10 }));
    resolvePipelineCacheKeys(ir2.graph, makeOpts({ now: 86400 * 11 }));

    const k1 = (ir1.graph.nodes[0].step.cache as Record<string, unknown>).key;
    const k2 = (ir2.graph.nodes[0].step.cache as Record<string, unknown>).key;
    expect(k1).not.toBe(k2);
  });

  it("ttl same bucket produces same key", () => {
    const ir1 = pipeline([sh("apt-get", { label: "apt", cache: ttl(86400) })]);
    const ir2 = pipeline([sh("apt-get", { label: "apt", cache: ttl(86400) })]);

    resolvePipelineCacheKeys(ir1.graph, makeOpts({ now: 86400 * 10 + 100 }));
    resolvePipelineCacheKeys(ir2.graph, makeOpts({ now: 86400 * 10 + 200 }));

    const k1 = (ir1.graph.nodes[0].step.cache as Record<string, unknown>).key;
    const k2 = (ir2.graph.nodes[0].step.cache as Record<string, unknown>).key;
    expect(k1).toBe(k2);
  });

  it("on_change hashes file contents", () => {
    const tmp = mkdtempSync(join(tmpdir(), "keygen-test-"));
    writeFileSync(join(tmp, "CMakeLists.txt"), "cmake_minimum_required(VERSION 3.20)");

    const ir = pipeline([
      sh("cmake ..", { label: "build", cache: onChange("./CMakeLists.txt") }),
    ]);
    resolvePipelineCacheKeys(ir.graph, makeOpts({ basePath: tmp }));

    const cache = ir.graph.nodes[0].step.cache as Record<string, unknown>;
    expect(cache.key).toBeTypeOf("string");
    expect((cache.key as string).length).toBe(64);
  });

  it("on_change different file contents produce different keys", () => {
    const tmp1 = mkdtempSync(join(tmpdir(), "keygen-test-"));
    const tmp2 = mkdtempSync(join(tmpdir(), "keygen-test-"));
    writeFileSync(join(tmp1, "f.txt"), "version A");
    writeFileSync(join(tmp2, "f.txt"), "version B");

    const ir1 = pipeline([sh("cmd", { label: "x", cache: onChange("./f.txt") })]);
    const ir2 = pipeline([sh("cmd", { label: "x", cache: onChange("./f.txt") })]);

    resolvePipelineCacheKeys(ir1.graph, makeOpts({ basePath: tmp1 }));
    resolvePipelineCacheKeys(ir2.graph, makeOpts({ basePath: tmp2 }));

    const k1 = (ir1.graph.nodes[0].step.cache as Record<string, unknown>).key;
    const k2 = (ir2.graph.nodes[0].step.cache as Record<string, unknown>).key;
    expect(k1).not.toBe(k2);
  });

  it("forever key matches Python algorithm", () => {
    const ir = pipeline([sh("echo hi", { label: "test", cache: forever() })]);
    const opts = makeOpts({ pipelineOrg: "myorg", pipelineSlug: "myslug" });

    resolvePipelineCacheKeys(ir.graph, opts);

    const stepKey = ir.graph.nodes[0].step.key as string;
    const cmd = "echo hi";
    const policyRes = "forever-" + sha256(cmd + NUL + "");
    const expected = sha256(
      "myorg" + NUL + "myslug" + NUL + stepKey + NUL + "scratch" + NUL + policyRes,
    );

    const cache = ir.graph.nodes[0].step.cache as Record<string, unknown>;
    expect(cache.key).toBe(expected);
  });

  it("child step uses parent resolved key", () => {
    const base = sh("apt-get install", { label: "apt", cache: forever() });
    const child = base.sh("make", { label: "build", cache: forever() });

    const ir = pipeline([child]);
    resolvePipelineCacheKeys(ir.graph, makeOpts());

    const parentCache = ir.graph.nodes[0].step.cache as Record<string, unknown>;
    const childCache = ir.graph.nodes[1].step.cache as Record<string, unknown>;
    expect(parentCache.key).toBeTypeOf("string");
    expect(childCache.key).toBeTypeOf("string");
    expect(parentCache.key).not.toBe(childCache.key);
  });

  it("golden hash: cross-SDK reference pipeline", () => {
    const graph: PipelineIR["graph"] = {
      nodes: [
        {
          step: {
            key: "build",
            cmd: "make build",
            cache: { policy: "forever", env_keys: [] },
          },
          env: {},
        },
      ],
      node_holes: [],
      edge_property: "directed",
      edges: [],
    };

    const opts: CacheKeyOptions = {
      pipelineOrg: "acme",
      pipelineSlug: "ci",
      now: 1000000,
      basePath: "/nonexistent",
      env: {},
    };

    resolvePipelineCacheKeys(graph, opts);

    const policyRes = "forever-" + sha256("make build" + NUL);
    const expected = sha256(
      "acme" + NUL + "ci" + NUL + "build" + NUL + "scratch" + NUL + policyRes,
    );

    const cache = graph.nodes[0].step.cache as Record<string, unknown>;
    expect(cache.key).toBe(expected);
  });

  it("golden hash: cross-SDK chained pipeline", () => {
    const graph: PipelineIR["graph"] = {
      nodes: [
        {
          step: {
            key: "setup",
            cmd: "apt-get update && apt-get install -y gcc",
            cache: { policy: "forever", env_keys: [] },
          },
          env: {},
        },
        {
          step: {
            key: "compile",
            cmd: "gcc -o main main.c",
            cache: { policy: "forever", env_keys: [] },
          },
          env: {},
        },
      ],
      node_holes: [],
      edge_property: "directed",
      edges: [[0, 1, "builds_in"]],
    };

    const opts: CacheKeyOptions = {
      pipelineOrg: "acme",
      pipelineSlug: "ci",
      now: 1000000,
      basePath: "/nonexistent",
      env: {},
    };

    resolvePipelineCacheKeys(graph, opts);

    const parentPolicyRes =
      "forever-" + sha256("apt-get update && apt-get install -y gcc" + NUL);
    const parentKey = sha256(
      "acme" + NUL + "ci" + NUL + "setup" + NUL + "scratch" + NUL + parentPolicyRes,
    );

    const childPolicyRes = "forever-" + sha256("gcc -o main main.c" + NUL);
    const childKey = sha256(
      "acme" + NUL + "ci" + NUL + "compile" + NUL + parentKey + NUL + childPolicyRes,
    );

    const parentCache = graph.nodes[0].step.cache as Record<string, unknown>;
    const childCache = graph.nodes[1].step.cache as Record<string, unknown>;
    expect(parentCache.key).toBe(parentKey);
    expect(childCache.key).toBe(childKey);
  });
});
