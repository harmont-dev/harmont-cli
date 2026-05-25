import { describe, expect, it } from "vitest";
import { py } from "../../../src/toolchains/index.js";
import { sh } from "../../../src/step.js";
import { pipeline } from "../../../src/pipeline.js";

describe("py.uv factory", () => {
  it("returns a UvProject with defaults", () => {
    const p = py.uv();
    expect(p.path).toBe(".");
    expect(p.install()._cmd).toContain("uv sync");
  });

  it("accepts path and version", () => {
    const p = py.uv({ path: "backend", version: "0.4.18" });
    expect(p.path).toBe("backend");
    expect(p.install()._parent!._cmd).toContain("UV_VERSION=0.4.18");
  });

  it("rejects invalid version", () => {
    expect(() => py.uv({ version: "abc" })).toThrow("invalid version");
  });

  it("latest omits UV_VERSION env prefix", () => {
    const p = py.uv({ version: "latest" });
    expect(p.install()._parent!._cmd).not.toContain("UV_VERSION");
  });
});

describe("py.uv actions", () => {
  it("test runs uv run pytest", () => {
    const p = py.uv();
    expect(p.test()._cmd).toContain("uv run pytest");
  });

  it("lint runs uv run ruff check", () => {
    const p = py.uv();
    expect(p.lint()._cmd).toContain("uv run ruff check .");
  });

  it("fmt runs uv run ruff format --check", () => {
    const p = py.uv();
    expect(p.fmt()._cmd).toContain("uv run ruff format --check .");
  });

  it("typecheck runs uv run ty check", () => {
    const p = py.uv();
    expect(p.typecheck()._cmd).toContain("uv run ty check .");
  });

  it("typecheck with paths string", () => {
    const p = py.uv({ path: "myapp" });
    expect(p.typecheck({ paths: "src" })._cmd).toContain("uv run ty check src");
  });

  it("typecheck with paths array", () => {
    const p = py.uv({ path: "myapp" });
    expect(p.typecheck({ paths: ["src", "tests"] })._cmd).toContain(
      "uv run ty check src tests",
    );
  });

  it("build runs uv build", () => {
    const p = py.uv();
    expect(p.build()._cmd).toContain("uv build");
  });

  it("lockCheck runs uv lock --check", () => {
    const p = py.uv();
    expect(p.lockCheck()._cmd).toContain("uv lock --check");
  });

  it("publish runs uv publish", () => {
    const p = py.uv();
    expect(p.publish()._cmd).toContain("uv publish");
  });

  it("run executes arbitrary command via uv run", () => {
    const p = py.uv();
    expect(p.run("flask db upgrade")._cmd).toContain("uv run flask db upgrade");
  });

  it("run auto-labels with first word of cmd", () => {
    const p = py.uv();
    expect(p.run("flask db upgrade")._label).toBe(":python: flask");
  });

  it("actions chain from install (sync step)", () => {
    const p = py.uv();
    expect(p.test()._parent).toBe(p.install());
  });

  it("default labels use :python: prefix", () => {
    const p = py.uv();
    expect(p.test()._label).toBe(":python: test");
    expect(p.lint()._label).toBe(":python: lint");
    expect(p.fmt()._label).toBe(":python: fmt");
    expect(p.typecheck()._label).toBe(":python: typecheck");
    expect(p.build()._label).toBe(":python: build");
    expect(p.lockCheck()._label).toBe(":python: lock-check");
    expect(p.publish()._label).toBe(":python: publish");
  });

  it("label override works", () => {
    const p = py.uv();
    expect(p.test({ label: "custom" })._label).toBe("custom");
  });
});

describe("py.uv install chain", () => {
  it("chain is: scratch -> apt-base -> uv-install -> uv-sync", () => {
    const p = py.uv();
    const sync = p.install();
    expect(sync._label).toBe(":python: uv-sync");

    const uvInstall = sync._parent!;
    expect(uvInstall._label).toBe(":python: uv-install");

    const aptBase = uvInstall._parent!;
    expect(aptBase._cmd).toContain("apt-get");
  });

  it("accepts base step", () => {
    const base = sh("custom");
    const p = py.uv({ base });
    const sync = p.install();
    const uvInstall = sync._parent!;
    expect(uvInstall._parent).toBe(base);
  });
});

describe("py.uv in pipeline", () => {
  it("produces valid IR", () => {
    const p = py.uv();
    const ir = pipeline(p.test(), p.lint(), { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
    expect(ir.version).toBe("0");
  });
});
