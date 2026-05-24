import { describe, expect, it } from "vitest";
import { npm } from "../../src/toolchains/index.js";
import { sh } from "../../src/step.js";
import { pipeline } from "../../src/pipeline.js";

describe("npm factory", () => {
  it("returns an NpmProject with install chain", () => {
    const n = npm();
    expect(n.path).toBe(".");
    const installed = n.install();
    expect(installed._cmd).toContain("npm ci");
  });

  it("accepts path and version", () => {
    const n = npm({ path: "packages/app", version: "22" });
    expect(n.path).toBe("packages/app");
    expect(n.install()._cmd).toContain("packages/app");
  });

  it("rejects invalid version", () => {
    expect(() => npm({ version: "abc" })).toThrow("invalid version");
  });

  it("accepts version with .x suffix", () => {
    expect(() => npm({ version: "20.x" })).not.toThrow();
  });
});

describe("npm actions", () => {
  it("test returns a step chained from install", () => {
    const n = npm();
    const t = n.test();
    expect(t._cmd).toContain("npm test");
    expect(t._parent).toBe(n.install());
  });

  it("lint runs npm run lint", () => {
    const n = npm();
    const l = n.lint();
    expect(l._cmd).toContain("npm run lint");
  });

  it("run executes arbitrary script", () => {
    const n = npm();
    const r = n.run("typecheck");
    expect(r._cmd).toContain("npm run typecheck");
  });

  it("actions accept step options", () => {
    const n = npm();
    const t = n.test({ label: "my test", timeoutSeconds: 300 });
    expect(t._label).toBe("my test");
    expect(t._timeoutSeconds).toBe(300);
  });

  it("default labels use :node: prefix", () => {
    const n = npm();
    expect(n.test()._label).toBe(":node: test");
    expect(n.lint()._label).toBe(":node: lint");
  });
});

describe("npm install chain structure", () => {
  it("chain is: scratch → apt-base → node-install → npm-ci", () => {
    const n = npm();
    const npmCi = n.install();
    expect(npmCi._cmd).toContain("npm ci");

    const nodeInstall = npmCi._parent!;
    expect(nodeInstall._cmd).toContain("nodejs");
    expect(nodeInstall._cache).toBeDefined();

    const aptBase = nodeInstall._parent!;
    expect(aptBase._cmd).toContain("apt-get");

    const root = aptBase._parent!;
    expect(root._cmd).toBeNull(); // scratch
  });

  it("accepts base step to skip apt chain", () => {
    const customBase = sh("custom base");
    const n = npm({ base: customBase });
    const npmCi = n.install();
    const nodeInstall = npmCi._parent!;
    // When base is provided, it's used directly
    expect(nodeInstall._parent).toBe(customBase);
  });

  it("accepts custom image", () => {
    const n = npm({ image: "debian:12" });
    const npmCi = n.install();
    const nodeInstall = npmCi._parent!;
    const aptBase = nodeInstall._parent!;
    const root = aptBase._parent!;
    expect(root._image).toBe("debian:12");
  });
});

describe("npm in pipeline", () => {
  it("produces valid IR when used as pipeline leaves", () => {
    const n = npm();
    const ir = pipeline(n.test(), n.lint(), { defaultImage: "ubuntu:24.04" });
    // Should have at least: apt-base, node-install, npm-ci, test, lint
    // (test and lint share npm-ci as parent)
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
    expect(ir.version).toBe("0");
  });
});
