import { describe, expect, it } from "vitest";
import { pipeline } from "../src/pipeline.js";
import { scratch, sh, wait } from "../src/step.js";
import { forever, onChange } from "../src/cache.js";

function stepKeys(ir: any): string[] {
  return ir.graph.nodes.map((n: any) => n.step.key);
}

function buildsInEdges(ir: any): [number, number][] {
  return ir.graph.edges
    .filter((e: any) => e[2] === "builds_in")
    .map((e: any) => [e[0], e[1]]);
}

function dependsOnEdges(ir: any): [number, number][] {
  return ir.graph.edges
    .filter((e: any) => e[2] === "depends_on")
    .map((e: any) => [e[0], e[1]]);
}

function parentKeyMap(ir: any): Record<string, string | null> {
  const keyByIdx: Record<number, string> = {};
  for (let i = 0; i < ir.graph.nodes.length; i++) {
    keyByIdx[i] = ir.graph.nodes[i].step.key;
  }
  const result: Record<string, string | null> = {};
  for (const n of ir.graph.nodes) {
    result[n.step.key] = null;
  }
  for (const [src, dst, kind] of ir.graph.edges) {
    if (kind === "builds_in") {
      result[keyByIdx[dst]] = keyByIdx[src];
    }
  }
  return result;
}

describe("pipeline", () => {
  it("returns v0 IR dict", () => {
    const p = pipeline([scratch().sh("echo", { label: "echo" })]);
    expect(p.version).toBe("0");
    expect(p.graph).toBeDefined();
    expect(p.graph.nodes).toHaveLength(1);
  });

  it("rejects no leaves", () => {
    expect(() => pipeline([])).toThrow("at least one leaf");
  });

  it("sets default_image on IR when provided", () => {
    const p = pipeline([sh("echo", { label: "a", image: "ubuntu:24.04" })], {
      defaultImage: "alpine:3.20",
    });
    expect(p.default_image).toBe("alpine:3.20");
    expect(p.graph.nodes[0].step.image).toBe("ubuntu:24.04");
  });
});

describe("lowering: single chain", () => {
  it("emits nodes in parent-first order with builds_in edges", () => {
    const a = scratch().sh("step a", { label: "a" });
    const b = a.sh("step b", { label: "b" });
    const c = b.sh("step c", { label: "c" });
    const ir = pipeline([c]);
    expect(stepKeys(ir)).toEqual(["a", "b", "c"]);
    const parents = parentKeyMap(ir);
    expect(parents.a).toBeNull();
    expect(parents.b).toBe("a");
    expect(parents.c).toBe("b");
  });
});

describe("lowering: fork", () => {
  it("fork nodes are not emitted, children inherit grandparent", () => {
    const base = scratch().sh("install", { label: "install" });
    const branch = base.fork({ label: "branch-a" });
    const leaf = branch.sh("test", { label: "test" });
    const ir = pipeline([leaf]);
    expect(stepKeys(ir)).toEqual(["install", "test"]);
    const parents = parentKeyMap(ir);
    expect(parents.install).toBeNull();
    expect(parents.test).toBe("install");
  });

  it("two branches share parent", () => {
    const base = scratch().sh("install", { label: "install" });
    const a = base.fork().sh("test-a", { label: "test-a" });
    const b = base.fork().sh("test-b", { label: "test-b" });
    const ir = pipeline([a, b]);
    const parents = parentKeyMap(ir);
    expect(parents["test-a"]).toBe("install");
    expect(parents["test-b"]).toBe("install");
  });
});

describe("lowering: wait", () => {
  it("emits depends_on edges from pre-wait to post-wait steps", () => {
    const a = scratch().sh("a", { label: "a" });
    const b = scratch().sh("b", { label: "b" });
    const c = scratch().sh("c", { label: "c" });
    const ir = pipeline([a, b, wait(), c]);
    const keys = stepKeys(ir);
    const idxA = keys.indexOf("a");
    const idxB = keys.indexOf("b");
    const idxC = keys.indexOf("c");
    const deps = dependsOnEdges(ir);
    expect(deps).toContainEqual([idxA, idxC]);
    expect(deps).toContainEqual([idxB, idxC]);
  });
});

describe("lowering: env merge", () => {
  it("merges pipeline env with per-step env", () => {
    const s = scratch().sh("make", { env: { STEP: "1" } });
    const ir = pipeline([s], { env: { PIPE: "true" } });
    expect(ir.graph.nodes[0].env).toEqual({ PIPE: "true", STEP: "1" });
  });

  it("step env overrides pipeline env", () => {
    const s = scratch().sh("make", { env: { X: "step" } });
    const ir = pipeline([s], { env: { X: "pipe" } });
    expect(ir.graph.nodes[0].env.X).toBe("step");
  });
});

describe("lowering: optional fields", () => {
  it("omits label/timeout/cache when unset", () => {
    const s = scratch().sh("make");
    const ir = pipeline([s]);
    const step = ir.graph.nodes[0].step;
    expect(step.key).toBeDefined();
    expect(step.cmd).toBe("make");
    expect("label" in step).toBe(false);
    expect("timeout_seconds" in step).toBe(false);
    expect("cache" in step).toBe(false);
  });

  it("includes label/timeout/cache when set", () => {
    const s = scratch().sh("make", {
      label: "build",
      timeoutSeconds: 600,
      cache: forever(),
    });
    const ir = pipeline([s]);
    const step = ir.graph.nodes[0].step;
    expect(step.label).toBe("build");
    expect(step.timeout_seconds).toBe(600);
    expect(step.cache).toEqual({ policy: "forever", env_keys: [] });
  });
});

describe("lowering: cache serialization", () => {
  it("serializes forever cache", () => {
    const s = sh("echo", { cache: forever({ envKeys: ["CI"] }) });
    const ir = pipeline([s]);
    expect(ir.graph.nodes[0].step.cache).toEqual({
      policy: "forever",
      env_keys: ["CI"],
    });
  });

  it("serializes onChange cache", () => {
    const s = sh("echo", { cache: onChange("src/", "lib/") });
    const ir = pipeline([s]);
    expect(ir.graph.nodes[0].step.cache).toEqual({
      policy: "on_change",
      paths: ["src/", "lib/"],
    });
  });
});

describe("lowering: dedup", () => {
  it("shared ancestor appears once when reachable from multiple leaves", () => {
    const base = scratch().sh("install", { label: "install" });
    const a = base.sh("a", { label: "a" });
    const b = base.sh("b", { label: "b" });
    const ir = pipeline([a, b]);
    const keys = stepKeys(ir);
    expect(keys.filter((k) => k === "install")).toHaveLength(1);
  });
});

describe("lowering: default_image", () => {
  it("applies default_image to root nodes without explicit image", () => {
    const s = scratch().sh("echo");
    const ir = pipeline([s], { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes[0].step.image).toBe("ubuntu:24.04");
  });

  it("does not override explicit image", () => {
    const s = scratch().sh("echo", { image: "alpine:3.20" });
    const ir = pipeline([s], { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes[0].step.image).toBe("alpine:3.20");
  });

  it("does not apply to child nodes with builds_in parent", () => {
    const parent = scratch().sh("a", { label: "a" });
    const child = parent.sh("b", { label: "b" });
    const ir = pipeline([child], { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes[0].step.image).toBe("ubuntu:24.04");
    expect("image" in ir.graph.nodes[1].step).toBe(false);
  });
});

describe("lowering: graph structure", () => {
  it("emits petgraph-serde structure", () => {
    const s = scratch().sh("echo", { label: "hello" });
    const ir = pipeline([s]);
    expect(ir.graph.node_holes).toEqual([]);
    expect(ir.graph.edge_property).toBe("directed");
  });
});
