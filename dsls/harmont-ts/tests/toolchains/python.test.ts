import { describe, expect, it } from "vitest";
import { python } from "../../src/toolchains/python.js";
import { sh } from "../../src/step.js";
import { pipeline } from "../../src/pipeline.js";

describe("python factory", () => {
  it("returns a PythonToolchain with defaults", () => {
    const p = python();
    expect(p.path).toBe(".");
    expect(p.install()._cmd).toContain("uv sync");
  });

  it("accepts path and uvVersion", () => {
    const p = python({ path: "backend", uvVersion: "0.2.0" });
    expect(p.path).toBe("backend");
    expect(p.install()._parent!._cmd).toContain("UV_VERSION=0.2.0");
  });

  it("rejects invalid uvVersion", () => {
    expect(() => python({ uvVersion: "abc" })).toThrow("invalid uv version");
  });

  it("latest uvVersion omits UV_VERSION env prefix", () => {
    const p = python({ uvVersion: "latest" });
    expect(p.install()._parent!._cmd).not.toContain("UV_VERSION");
  });
});

describe("python actions", () => {
  it("test runs uv run pytest", () => {
    const p = python();
    expect(p.test()._cmd).toContain("uv run pytest");
  });

  it("lint runs uv run ruff check", () => {
    const p = python();
    expect(p.lint()._cmd).toContain("uv run ruff check .");
  });

  it("fmt runs uv run ruff format --check", () => {
    const p = python();
    expect(p.fmt()._cmd).toContain("uv run ruff format --check .");
  });

  it("typecheck runs uv run mypy", () => {
    const p = python();
    expect(p.typecheck()._cmd).toContain("uv run mypy .");
  });

  it("typecheck with paths string", () => {
    const p = python({ path: "myapp" });
    expect(p.typecheck({ paths: "src" })._cmd).toContain("uv run mypy src");
  });

  it("typecheck with paths array", () => {
    const p = python({ path: "myapp" });
    expect(p.typecheck({ paths: ["src", "tests"] })._cmd).toContain(
      "uv run mypy src tests",
    );
  });

  it("actions chain from install (sync step)", () => {
    const p = python();
    expect(p.test()._parent).toBe(p.install());
  });

  it("default labels use :python: prefix", () => {
    const p = python();
    expect(p.test()._label).toBe(":python: test");
    expect(p.lint()._label).toBe(":python: lint");
    expect(p.fmt()._label).toBe(":python: fmt");
    expect(p.typecheck()._label).toBe(":python: typecheck");
  });
});

describe("python install chain", () => {
  it("chain is: scratch → apt-base → uv-install → uv-sync", () => {
    const p = python();
    const sync = p.install();
    expect(sync._label).toBe(":python: uv-sync");

    const uvInstall = sync._parent!;
    expect(uvInstall._label).toBe(":python: uv-install");

    const aptBase = uvInstall._parent!;
    expect(aptBase._cmd).toContain("apt-get");
  });

  it("accepts base step", () => {
    const base = sh("custom");
    const p = python({ base });
    const sync = p.install();
    const uvInstall = sync._parent!;
    expect(uvInstall._parent).toBe(base);
  });
});

describe("python in pipeline", () => {
  it("produces valid IR", () => {
    const p = python();
    const ir = pipeline(p.test(), p.lint(), { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
    expect(ir.version).toBe("0");
  });
});
