import { describe, expect, it } from "vitest";
import { cmake, CMakeToolchain, CMakeProject } from "../../src/toolchains/cmake.js";
import { pipeline } from "../../src/pipeline.js";

describe("cmake factory", () => {
  it("returns a CMakeToolchain when no path is given", () => {
    const tc = cmake();
    expect(tc).toBeInstanceOf(CMakeToolchain);
    expect(tc.install()._cmd).toContain("cmake --version");
    expect(tc.install()._cmd).toContain("ninja --version");
  });

  it("returns a CMakeProject when path is given", () => {
    const proj = cmake({ path: "." });
    expect(proj).toBeInstanceOf(CMakeProject);
    expect(proj.path).toBe(".");
  });

  it("rejects invalid compiler", () => {
    expect(() => cmake({ compiler: "msvc" })).toThrow("invalid compiler");
  });

  it("rejects invalid generator", () => {
    expect(() => cmake({ generator: "borland" as any })).toThrow(
      "invalid generator",
    );
  });

  it("accepts gcc compiler", () => {
    const tc = cmake({ compiler: "gcc-14" });
    expect(tc.install()._cmd).toContain("gcc-14 --version");
  });

  it("accepts clang compiler", () => {
    const tc = cmake({ compiler: "clang-18" });
    expect(tc.install()._cmd).toContain("clang-18 --version");
  });

  it("disables ccache", () => {
    const tc = cmake({ ccache: false });
    expect(tc.install()._cmd).not.toContain("ccache");
  });
});

describe("cmake toolchain.project()", () => {
  it("creates a CMakeProject from a toolchain", () => {
    const tc = cmake();
    const proj = tc.project({ path: "lib" });
    expect(proj).toBeInstanceOf(CMakeProject);
    expect(proj.path).toBe("lib");
  });
});

describe("cmake project actions", () => {
  it("build returns the warmup step", () => {
    const proj = cmake({ path: "." });
    expect(proj.build()._cmd).toContain("cmake --build");
    expect(proj.build()._label).toBe(":cmake: build");
  });

  it("test runs ctest off built", () => {
    const proj = cmake({ path: "." });
    const step = proj.test();
    expect(step._cmd).toContain("ctest --test-dir ./build --output-on-failure");
    expect(step._label).toBe(":cmake: test");
  });

  it("install runs cmake --install off built", () => {
    const proj = cmake({ path: "." });
    const step = proj.install();
    expect(step._cmd).toContain("cmake --install ./build");
    expect(step._label).toBe(":cmake: install");
  });

  it("fmt runs clang-format off toolchain.install()", () => {
    const proj = cmake({ path: "." });
    const step = proj.fmt();
    expect(step._cmd).toContain("clang-format --dry-run --Werror");
    expect(step._label).toBe(":cmake: fmt");
    // fmt branches off toolchain.install(), not built
    expect(step._parent).toBe(proj.toolchain.install());
  });

  it("lint runs run-clang-tidy off built", () => {
    const proj = cmake({ path: "." });
    const step = proj.lint();
    expect(step._cmd).toContain("run-clang-tidy -p build");
    expect(step._label).toBe(":cmake: lint");
  });

  it("package runs cpack off built", () => {
    const proj = cmake({ path: "." });
    const step = proj.package();
    expect(step._cmd).toContain("cpack");
    expect(step._label).toBe(":cmake: package");
  });
});

describe("cmake with preset", () => {
  it("configure uses --preset when preset is given", () => {
    const proj = cmake({ path: ".", preset: "release" });
    expect(proj.build()._cmd).toContain("cmake --preset release");
    expect(proj.build()._cmd).not.toContain("-DCMAKE_BUILD_TYPE");
  });
});

describe("cmake with vcpkg", () => {
  it("inserts a vcpkg step", () => {
    const proj = cmake({ path: ".", deps: "vcpkg" });
    // The built step's parent should be the vcpkg step (warmup parent)
    const builtStep = proj.build();
    expect(builtStep._parent?._cmd).toContain("vcpkg install");
    expect(builtStep._parent?._label).toBe(":cmake: vcpkg");
  });
});

describe("cmake in pipeline", () => {
  it("produces valid IR", () => {
    const proj = cmake({ path: "." });
    const ir = pipeline([proj.build(), proj.test()]);
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(3);
  });
});
