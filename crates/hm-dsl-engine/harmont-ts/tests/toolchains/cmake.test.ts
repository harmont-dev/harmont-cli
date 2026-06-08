import { describe, expect, it } from "vitest";
import { cmake } from "../../src/toolchains/cmake.js";
import { pipeline } from "../../src/pipeline.js";

describe("cmake factory", () => {
  it("returns a CMakeProject with defaults", () => {
    const c = cmake();
    expect(c.path).toBe(".");
    expect(c.install()._cmd).toContain("cmake --version");
  });

  it("accepts lang cpp", () => {
    const c = cmake({ lang: "cpp" });
    expect(c.build()._label).toBe(":cpp: build");
  });

  it("rejects invalid lang", () => {
    expect(() => cmake({ lang: "java" as any })).toThrow("invalid lang");
  });
});

describe("cmake actions", () => {
  it("configure runs cmake -S . -B build", () => {
    expect(cmake().configure()._cmd).toContain("cmake -S . -B build");
  });

  it("build runs cmake --build", () => {
    expect(cmake().build()._cmd).toContain("cmake --build build");
  });

  it("test runs ctest", () => {
    expect(cmake().test()._cmd).toContain("ctest --test-dir build");
  });

  it("fmt runs clang-format", () => {
    expect(cmake().fmt()._cmd).toContain("clang-format --dry-run --Werror");
  });

  it("labels use lang tag", () => {
    const c = cmake({ lang: "c" });
    expect(c.build()._label).toBe(":c: build");

    const cpp = cmake({ lang: "cpp" });
    expect(cpp.build()._label).toBe(":cpp: build");
  });
});

describe("cmake in pipeline", () => {
  it("produces valid IR", () => {
    const c = cmake();
    const ir = pipeline([c.build(), c.test()], { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(3);
  });
});
