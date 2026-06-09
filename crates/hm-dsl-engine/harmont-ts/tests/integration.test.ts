import { describe, expect, it, beforeEach } from "vitest";
import {
  Step,
  scratch,
  sh,
  wait,
  forever,
  ttl,
  onChange,
  compose,
  pipeline,
  target,
  clearTargetCache,
  renderEnvelope,
  push,
  pullRequest,
  PushTrigger,
  PullRequestTrigger,
  type PipelineDefinition,
} from "../src/index.js";

beforeEach(() => {
  clearTargetCache();
});

describe("full pipeline build", () => {
  it("creates install -> build -> test chain with cache, env, defaultImage", () => {
    const install = scratch()
      .sh("npm ci", { label: "install", cache: forever() });
    const build = install
      .sh("npm run build", { label: "build", env: { NODE_ENV: "production" } });
    const test = build
      .sh("npm test", { label: "test", timeoutSeconds: 300 });

    const ir = pipeline([test], {
      env: { CI: "true" },
      defaultImage: "node:22-alpine",
    });

    // version
    expect(ir.version).toBe("0");

    // node count
    expect(ir.graph.nodes).toHaveLength(3);

    // edge count — two builds_in edges (install->build, build->test)
    expect(ir.graph.edges).toHaveLength(2);
    expect(ir.graph.edges.every((e) => e[2] === "builds_in")).toBe(true);

    // env merge: pipeline env CI=true merges with step env
    const installNode = ir.graph.nodes[0];
    const buildNode = ir.graph.nodes[1];
    const testNode = ir.graph.nodes[2];

    expect(installNode.env).toEqual({ CI: "true" });
    expect(buildNode.env).toEqual({ CI: "true", NODE_ENV: "production" });
    expect(testNode.env).toEqual({ CI: "true" });

    // default_image applies to root node only (install), not children
    expect(ir.default_image).toBe("node:22-alpine");
    expect(installNode.step.image).toBe("node:22-alpine");
    expect("image" in buildNode.step).toBe(false);
    expect("image" in testNode.step).toBe(false);

    // cache on install step
    expect(installNode.step.cache).toEqual({ policy: "forever", env_keys: [] });

    // timeout on test step
    expect(testNode.step.timeout_seconds).toBe(300);
  });
});

describe("wait barrier", () => {
  it("creates depends_on edges from pre-wait steps to post-wait steps", () => {
    const a = scratch().sh("step a", { label: "a" });
    const b = scratch().sh("step b", { label: "b" });
    const c = scratch().sh("step c", { label: "c" });
    const ir = pipeline([a, b, wait(), c]);

    const keys = ir.graph.nodes.map((n) => n.step.key);
    const idxA = keys.indexOf("a");
    const idxB = keys.indexOf("b");
    const idxC = keys.indexOf("c");

    const dependsOnEdges = ir.graph.edges.filter((e) => e[2] === "depends_on");

    // c depends_on both a and b
    expect(dependsOnEdges).toContainEqual([idxA, idxC, "depends_on"]);
    expect(dependsOnEdges).toContainEqual([idxB, idxC, "depends_on"]);
    expect(dependsOnEdges).toHaveLength(2);
  });
});

describe("target memoization", () => {
  it("shared target appears once in graph when used in two branches", () => {
    const nodeBase = target("node-base", () =>
      sh("apt-get install -y nodejs", {
        label: "node-base",
        cache: forever(),
      }),
    );

    const branchA = nodeBase().sh("npm run lint", { label: "lint" });
    const branchB = nodeBase().sh("npm test", { label: "test" });

    const ir = pipeline([branchA, branchB]);

    // node-base should appear exactly once (memoized)
    const keys = ir.graph.nodes.map((n) => n.step.key);
    expect(keys.filter((k) => k === "node-base")).toHaveLength(1);

    // total nodes: node-base, lint, test
    expect(ir.graph.nodes).toHaveLength(3);

    // both branches build from node-base
    const nodeBaseIdx = keys.indexOf("node-base");
    const lintIdx = keys.indexOf("lint");
    const testIdx = keys.indexOf("test");

    const buildsInEdges = ir.graph.edges.filter((e) => e[2] === "builds_in");
    expect(buildsInEdges).toContainEqual([nodeBaseIdx, lintIdx, "builds_in"]);
    expect(buildsInEdges).toContainEqual([nodeBaseIdx, testIdx, "builds_in"]);
  });
});

describe("envelope", () => {
  it("renders a complete envelope with triggers", () => {
    const def: PipelineDefinition = {
      slug: "my-ci",
      name: "My CI Pipeline",
      allowManual: false,
      triggers: [
        push({ branch: "main" }),
        pullRequest({ branches: "develop" }),
      ],
      pipeline: pipeline([sh("echo hello", { label: "hello" })]),
    };

    const json = renderEnvelope([def]);
    const parsed = JSON.parse(json);

    // schema_version
    expect(parsed.schema_version).toBe("1");

    // pipeline metadata
    expect(parsed.pipelines).toHaveLength(1);
    const p = parsed.pipelines[0];
    expect(p.slug).toBe("my-ci");
    expect(p.name).toBe("My CI Pipeline");
    expect(p.allow_manual).toBe(false);

    // triggers
    expect(p.triggers).toHaveLength(2);
    expect(p.triggers[0]).toEqual({ event: "push", branches: ["main"] });
    expect(p.triggers[1]).toEqual({
      event: "pull_request",
      branches: ["develop"],
      types: ["opened", "synchronize", "reopened"],
    });
    // definition is the IR
    expect(p.definition.version).toBe("0");
    expect(p.definition.graph.nodes).toHaveLength(1);
  });
});

describe("JSON snake_case output", () => {
  it("uses snake_case keys in IR, not camelCase", () => {
    const s = scratch().sh("make", {
      label: "build",
      timeoutSeconds: 600,
      cache: onChange("src/", "lib/"),
    });
    const ir = pipeline([s], { defaultImage: "ubuntu:24.04" });
    const json = JSON.stringify(ir);

    // Must contain snake_case keys
    expect(json).toContain('"default_image"');
    expect(json).toContain('"timeout_seconds"');
    expect(json).toContain('"edge_property"');
    expect(json).toContain('"node_holes"');
    expect(json).toContain('"on_change"');

    // Must NOT contain camelCase equivalents
    expect(json).not.toContain('"defaultImage"');
    expect(json).not.toContain('"timeoutSeconds"');
    expect(json).not.toContain('"edgeProperty"');
    expect(json).not.toContain('"nodeHoles"');
    expect(json).not.toContain('"onChange"');
  });

  it("envelope uses snake_case keys", () => {
    const def: PipelineDefinition = {
      slug: "ci",
      allowManual: true,
      pipeline: pipeline([sh("echo")]),
    };
    const json = renderEnvelope([def]);

    expect(json).toContain('"schema_version"');
    expect(json).toContain('"allow_manual"');
    expect(json).not.toContain('"schemaVersion"');
    expect(json).not.toContain('"allowManual"');
  });
});

describe("public API completeness", () => {
  it("exports all expected symbols", () => {
    // Classes and functions are values
    expect(Step).toBeDefined();
    expect(typeof scratch).toBe("function");
    expect(typeof sh).toBe("function");
    expect(typeof wait).toBe("function");
    expect(typeof forever).toBe("function");
    expect(typeof ttl).toBe("function");
    expect(typeof onChange).toBe("function");
    expect(typeof compose).toBe("function");
    expect(typeof pipeline).toBe("function");
    expect(typeof target).toBe("function");
    expect(typeof clearTargetCache).toBe("function");
    expect(typeof renderEnvelope).toBe("function");
    expect(typeof push).toBe("function");
    expect(typeof pullRequest).toBe("function");
    // Trigger classes are exported as values for instanceof checks
    expect(PushTrigger).toBeDefined();
    expect(PullRequestTrigger).toBeDefined();
    const t = push({ branch: "main" });
    expect(t instanceof PushTrigger).toBe(true);
  });
});
