import { describe, expect, it } from "vitest";
import { zig, ZigToolchain, ZigProject } from "../../src/toolchains/zig.js";
import { sh } from "../../src/step.js";
import { pipeline } from "../../src/pipeline.js";

describe("zig factory", () => {
  it("returns ZigToolchain without path", () => {
    const tc = zig();
    expect(tc).toBeInstanceOf(ZigToolchain);
  });

  it("returns ZigProject with path", () => {
    const proj = zig({ path: "." });
    expect(proj).toBeInstanceOf(ZigProject);
    expect(proj.path).toBe(".");
  });

  it("rejects invalid version", () => {
    expect(() => zig({ version: "abc" })).toThrow("invalid version");
  });
});

describe("zig toolchain", () => {
  it("project creates ZigProject sharing install step", () => {
    const tc = zig();
    const a = tc.project("lib-a");
    const b = tc.project("lib-b");
    expect(a.install()).toBe(b.install());
    expect(a.path).toBe("lib-a");
    expect(b.path).toBe("lib-b");
  });
});

describe("zig project actions", () => {
  it("build runs zig build", () => {
    const p = zig({ path: "." });
    expect(p.build()._cmd).toContain("zig build");
  });

  it("test runs zig build test", () => {
    const p = zig({ path: "." });
    expect(p.test()._cmd).toContain("zig build test");
  });

  it("fmt runs zig fmt --check", () => {
    const p = zig({ path: "." });
    expect(p.fmt()._cmd).toContain("zig fmt --check");
  });

  it("labels include project path", () => {
    const p = zig({ path: "lib-a" });
    expect(p.build()._label).toBe(":zig: lib-a build");
    expect(p.test()._label).toBe(":zig: lib-a test");
  });
});

describe("zig install chain", () => {
  it("chain is: scratch → apt-base → zig-install", () => {
    const tc = zig();
    const install = tc.install();
    expect(install._cmd).toContain("zig version");

    const aptBase = install._parent!;
    expect(aptBase._cmd).toContain("apt-get");
  });

  it("accepts base step", () => {
    const base = sh("custom");
    const tc = zig({ base });
    expect(tc.install()._parent).toBe(base);
  });
});

describe("zig multi-project pipeline", () => {
  it("two projects share one install step in IR", () => {
    const tc = zig();
    const a = tc.project("lib-a");
    const b = tc.project("lib-b");
    const ir = pipeline(a.build(), a.test(), b.build(), b.test(), {
      defaultImage: "ubuntu:24.04",
    });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(5);
    expect(ir.version).toBe("0");
  });
});
