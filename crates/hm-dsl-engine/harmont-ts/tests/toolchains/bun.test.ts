import { describe, expect, it } from "vitest";
import { bun } from "../../src/toolchains/index.js";
import { sh, timeout } from "../../src/step.js";
import { pipeline } from "../../src/pipeline.js";

describe("bun factory", () => {
  it("returns a BunProject with install chain", () => {
    const b = bun();
    expect(b.path).toBe(".");
    const installed = b.install();
    expect(installed._cmd).toContain("bun install");
  });

  it("accepts path and version", () => {
    const b = bun({ path: "packages/app", version: "1.2.0" });
    expect(b.path).toBe("packages/app");
    expect(b.install()._cmd).toContain("packages/app");
  });

  it("rejects invalid version", () => {
    expect(() => bun({ version: "abc" })).toThrow("invalid version");
  });

  it("accepts two-part version", () => {
    expect(() => bun({ version: "1.2" })).not.toThrow();
  });
});

describe("bun actions", () => {
  it("test returns a step chained from install", () => {
    const b = bun();
    const t = b.test();
    expect(t._cmd).toContain("bun test");
    expect(t._parent).toBe(b.install());
  });

  it("lint runs bun run lint", () => {
    const b = bun();
    const l = b.lint();
    expect(l._cmd).toContain("bun run lint");
  });

  it("run executes arbitrary script", () => {
    const b = bun();
    const r = b.run("typecheck");
    expect(r._cmd).toContain("bun run typecheck");
  });

  it("actions accept step options", () => {
    const b = bun();
    const t = timeout(300, b.test({ label: "my test" }));
    expect(t._label).toBe("my test");
    expect(t._timeoutSeconds).toBe(300);
  });

  it("default labels use :bun: prefix", () => {
    const b = bun();
    expect(b.test()._label).toBe(":bun: test");
    expect(b.lint()._label).toBe(":bun: lint");
  });
});

describe("bun install chain structure", () => {
  it("chain is: scratch → apt-base → bun-install → bun-install-deps", () => {
    const b = bun();
    const bunInstallDeps = b.install();
    expect(bunInstallDeps._cmd).toContain("bun install");

    const bunSetup = bunInstallDeps._parent!;
    expect(bunSetup._cmd).toContain("bun.sh/install");
    expect(bunSetup._cache).toBeDefined();

    const aptBase = bunSetup._parent!;
    expect(aptBase._cmd).toContain("apt-get");
    expect(aptBase._cmd).toContain("unzip");

    const root = aptBase._parent!;
    expect(root._cmd).toBeNull(); // scratch
  });

  it("accepts base step to skip apt chain", () => {
    const customBase = sh("custom base");
    const b = bun({ base: customBase });
    const bunInstallDeps = b.install();
    const bunSetup = bunInstallDeps._parent!;
    expect(bunSetup._parent).toBe(customBase);
  });

  it("accepts custom image", () => {
    const b = bun({ image: "debian:12" });
    const bunInstallDeps = b.install();
    const bunSetup = bunInstallDeps._parent!;
    const aptBase = bunSetup._parent!;
    const root = aptBase._parent!;
    expect(root._image).toBe("debian:12");
  });
});

describe("bun in pipeline", () => {
  it("produces valid IR when used as pipeline leaves", () => {
    const b = bun();
    const ir = pipeline(b.test(), b.lint(), { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
    expect(ir.version).toBe("0");
  });
});
