import { describe, expect, it } from "vitest";
import { gradle } from "../../src/toolchains/gradle.js";
import { pipeline } from "../../src/pipeline.js";

describe("gradle factory", () => {
  it("returns a GradleProject with defaults", () => {
    const g = gradle();
    expect(g.path).toBe(".");
    expect(g.install()._cmd).toContain("gradle --version");
  });

  it("accepts jdk and kotlin flag", () => {
    const g = gradle({ jdk: "17", kotlin: true });
    expect(g.install()._parent!._cmd).toContain("openjdk-17");
    expect(g.build()._label).toBe(":kotlin: build");
  });

  it("rejects invalid jdk", () => {
    expect(() => gradle({ jdk: "8" })).toThrow("invalid jdk");
  });
});

describe("gradle actions", () => {
  it("build runs gradle build", () => {
    expect(gradle().build()._cmd).toContain("gradle build");
  });

  it("test runs gradle test", () => {
    expect(gradle().test()._cmd).toContain("gradle test");
  });

  it("lint runs gradle check", () => {
    expect(gradle().lint()._cmd).toContain("gradle check");
  });

  it("java labels use :java: prefix", () => {
    const g = gradle();
    expect(g.build()._label).toBe(":java: build");
  });

  it("kotlin labels use :kotlin: prefix", () => {
    const g = gradle({ kotlin: true });
    expect(g.build()._label).toBe(":kotlin: build");
  });
});

describe("gradle in pipeline", () => {
  it("produces valid IR", () => {
    const g = gradle();
    const ir = pipeline(g.build(), g.test(), { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(3);
  });
});
