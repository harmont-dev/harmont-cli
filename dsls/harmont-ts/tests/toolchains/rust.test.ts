import { describe, expect, it } from "vitest";
import { rust } from "../../src/toolchains/rust.js";
import { sh } from "../../src/step.js";
import { pipeline } from "../../src/pipeline.js";

describe("rust factory", () => {
  it("returns a RustToolchain with defaults", () => {
    const r = rust();
    expect(r.path).toBe(".");
    expect(r.install()._cmd).toContain("rustc --version");
  });

  it("accepts path and version", () => {
    const r = rust({ path: "crates/core", version: "nightly" });
    expect(r.path).toBe("crates/core");
    expect(r.install()._cmd).toContain("nightly");
  });

  it("accepts custom components", () => {
    const r = rust({ components: ["clippy", "rustfmt", "miri"] });
    expect(r.install()._cmd).toContain("clippy,rustfmt,miri");
  });

  it("rejects invalid version", () => {
    expect(() => rust({ version: "not valid!" })).toThrow("invalid version");
  });
});

describe("rust actions", () => {
  it("build runs cargo build", () => {
    const r = rust();
    expect(r.build()._cmd).toContain("cargo build");
    expect(r.build()._cmd).not.toContain("--release");
  });

  it("build --release", () => {
    const r = rust();
    expect(r.build({ release: true })._cmd).toContain("cargo build --release");
  });

  it("test runs cargo test", () => {
    const r = rust();
    expect(r.test()._cmd).toContain("cargo test");
  });

  it("clippy runs with -D warnings", () => {
    const r = rust();
    expect(r.clippy()._cmd).toContain("cargo clippy --all-targets -- -D warnings");
  });

  it("fmt runs cargo fmt --check", () => {
    const r = rust();
    expect(r.fmt()._cmd).toContain("cargo fmt --check");
  });

  it("doc runs cargo doc --no-deps", () => {
    const r = rust();
    expect(r.doc()._cmd).toContain("cargo doc --no-deps");
  });

  it("actions source cargo env", () => {
    const r = rust();
    expect(r.build()._cmd).toContain(". $HOME/.cargo/env");
  });

  it("actions chain from install", () => {
    const r = rust();
    expect(r.build()._parent).toBe(r.install());
  });

  it("accepts step options", () => {
    const r = rust();
    const t = r.test({ label: "my test", timeoutSeconds: 600 });
    expect(t._label).toBe("my test");
    expect(t._timeoutSeconds).toBe(600);
  });

  it("default labels use :rust: prefix", () => {
    const r = rust();
    expect(r.build()._label).toBe(":rust: build");
    expect(r.test()._label).toBe(":rust: test");
    expect(r.clippy()._label).toBe(":rust: clippy");
    expect(r.fmt()._label).toBe(":rust: fmt");
    expect(r.doc()._label).toBe(":rust: doc");
  });
});

describe("rust install chain", () => {
  it("chain is: scratch → apt-base → rustup", () => {
    const r = rust();
    const install = r.install();
    expect(install._label).toBe(":rust: rustup");

    const aptBase = install._parent!;
    expect(aptBase._cmd).toContain("apt-get");

    const root = aptBase._parent!;
    expect(root._cmd).toBeNull();
  });

  it("accepts base step", () => {
    const base = sh("custom base");
    const r = rust({ base });
    expect(r.install()._parent).toBe(base);
  });
});

describe("rust in pipeline", () => {
  it("produces valid IR", () => {
    const r = rust();
    const ir = pipeline(r.build(), r.test(), r.clippy(), r.fmt(), {
      defaultImage: "ubuntu:24.04",
    });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
    expect(ir.version).toBe("0");
  });
});
