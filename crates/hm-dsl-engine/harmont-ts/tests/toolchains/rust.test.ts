import { describe, expect, it } from "vitest";
import { rust } from "../../src/toolchains/rust.js";
import { sh, timeout } from "../../src/step.js";
import { pipeline } from "../../src/pipeline.js";

const cmds = (ir: ReturnType<typeof pipeline>) =>
  ir.graph.nodes.map((n: { step: { cmd: string } }) => n.step.cmd);

const stepBySubstring = (ir: ReturnType<typeof pipeline>, needle: string) => {
  const node = ir.graph.nodes.find((n: { step: { cmd: string } }) =>
    n.step.cmd.includes(needle),
  );
  if (!node) throw new Error(`no command step containing "${needle}"`);
  return node.step;
};

describe("rust.toolchain", () => {
  it("returns a RustToolchain with defaults", () => {
    const r = rust.toolchain();
    expect(r.path).toBe(".");
    expect(r.install()._cmd).toContain("rustc --version");
  });

  it("accepts path and version", () => {
    const r = rust.toolchain({ path: "crates/core", version: "nightly" });
    expect(r.path).toBe("crates/core");
    expect(r.install()._cmd).toContain("nightly");
  });

  it("accepts custom components", () => {
    const r = rust.toolchain({ components: ["clippy", "rustfmt", "miri"] });
    expect(r.install()._cmd).toContain("clippy,rustfmt,miri");
  });

  it("rejects invalid version", () => {
    expect(() => rust.toolchain({ version: "not valid!" })).toThrow(
      "invalid version",
    );
  });

  it("build runs cargo build", () => {
    const r = rust.toolchain();
    expect(r.build()._cmd).toContain("cargo build");
    expect(r.build()._cmd).not.toContain("--release");
  });

  it("build --release", () => {
    const r = rust.toolchain();
    expect(r.build({ release: true })._cmd).toContain(
      "cargo build --release",
    );
  });

  it("test runs cargo test", () => {
    const r = rust.toolchain();
    expect(r.test()._cmd).toContain("cargo test");
  });

  it("clippy runs with -D warnings", () => {
    const r = rust.toolchain();
    expect(r.clippy()._cmd).toContain(
      "cargo clippy --all-targets -- -D warnings",
    );
  });

  it("fmt runs cargo fmt --check", () => {
    const r = rust.toolchain();
    expect(r.fmt()._cmd).toContain("cargo fmt --check");
  });

  it("doc runs cargo doc --no-deps", () => {
    const r = rust.toolchain();
    expect(r.doc()._cmd).toContain("cargo doc --no-deps");
  });

  it("actions source cargo env", () => {
    const r = rust.toolchain();
    expect(r.build()._cmd).toContain(". $HOME/.cargo/env");
  });

  it("actions chain from install", () => {
    const r = rust.toolchain();
    expect(r.build()._parent).toBe(r.install());
  });

  it("accepts step options", () => {
    const r = rust.toolchain();
    const t = timeout(600, r.test({ label: "my test" }));
    expect(t._label).toBe("my test");
    expect(t._timeoutSeconds).toBe(600);
  });

  it("default labels use :rust: prefix", () => {
    const r = rust.toolchain();
    expect(r.build()._label).toBe(":rust: build");
    expect(r.test()._label).toBe(":rust: test");
    expect(r.clippy()._label).toBe(":rust: clippy");
    expect(r.fmt()._label).toBe(":rust: fmt");
    expect(r.doc()._label).toBe(":rust: doc");
  });

  it("warmup runs cargo build --workspace --tests --locked", () => {
    const r = rust.toolchain();
    expect(r.warmup()._cmd).toContain(
      "cargo build --workspace --tests --locked",
    );
  });

  it("warmup chains from install", () => {
    const r = rust.toolchain();
    expect(r.warmup()._parent).toBe(r.install());
  });

  it("warmup default label", () => {
    const r = rust.toolchain();
    expect(r.warmup()._label).toBe(":rust: warmup");
  });

  it("warmup accepts options", () => {
    const r = rust.toolchain();
    const w = r.warmup({ label: ":rust: pre-build" });
    expect(w._label).toBe(":rust: pre-build");
  });

  it("chain is: scratch → apt-base → rustup", () => {
    const r = rust.toolchain();
    const install = r.install();
    expect(install._label).toBe(":rust: rustup");

    const aptBase = install._parent!;
    expect(aptBase._cmd).toContain("apt-get");

    const root = aptBase._parent!;
    expect(root._cmd).toBeNull();
  });

  it("accepts base step", () => {
    const base = sh("custom base");
    const r = rust.toolchain({ base });
    expect(r.install()._parent).toBe(base);
  });

  it("produces valid pipeline IR", () => {
    const r = rust.toolchain();
    const ir = pipeline([r.build(), r.test(), r.clippy(), r.fmt()], {
      defaultImage: "ubuntu:24.04",
    });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
    expect(ir.version).toBe("0");
  });
});

describe("rust.project", () => {
  it("has all methods", () => {
    const proj = rust.project({ path: "cli" });
    expect(proj.warmup._cmd).toContain(
      "cargo build --workspace --tests --locked",
    );
    expect(proj.test()._cmd).toContain("cargo test --workspace --locked");
    expect(proj.clippy()._cmd).toContain(
      "cargo clippy --workspace --tests --locked",
    );
    expect(proj.fmt()._cmd).toContain("cargo fmt --check");
  });

  it("warmup has implicit CacheOnChange on Cargo.lock, Cargo.toml, and *.rs", () => {
    const proj = rust.project({ path: "cli" });
    expect(proj.warmup._cache).toEqual({
      kind: "on_change",
      paths: ["cli/Cargo.lock", "cli/**/Cargo.toml", "cli/**/*.rs"],
    });
  });

  it("warmup cache uses plain paths for dot path", () => {
    const proj = rust.project({ path: "." });
    expect(proj.warmup._cache).toEqual({
      kind: "on_change",
      paths: ["Cargo.lock", "**/Cargo.toml", "**/*.rs"],
    });
  });

  it("warmup cache can be overridden", () => {
    const proj = rust.project({
      path: ".",
      cache: { kind: "on_change", paths: ["Cargo.toml"] },
    });
    expect(proj.warmup._cache).toEqual({
      kind: "on_change",
      paths: ["Cargo.toml"],
    });
  });

  it("test flags are appended", () => {
    const proj = rust.project({ path: "." });
    expect(proj.test({ flags: ["--lib", "--no-fail-fast"] })._cmd).toContain(
      "cargo test --workspace --locked --lib --no-fail-fast",
    );
  });

  it("clippy flags are inserted before --", () => {
    const proj = rust.project({ path: "." });
    expect(proj.clippy({ flags: ["--fix"] })._cmd).toContain(
      "cargo clippy --workspace --tests --locked --fix -- -D warnings",
    );
  });

  it("fmt flags are appended", () => {
    const proj = rust.project({ path: "." });
    expect(proj.fmt({ flags: ["--all"] })._cmd).toContain(
      "cargo fmt --check --all",
    );
  });

  it("test chains off warmup", () => {
    const proj = rust.project();
    expect(proj.test()._parent).toBe(proj.warmup);
  });

  it("clippy chains off warmup", () => {
    const proj = rust.project();
    expect(proj.clippy()._parent).toBe(proj.warmup);
  });

  it("fmt chains off install (not warmup)", () => {
    const proj = rust.project();
    expect(proj.fmt()._parent).toBe(proj.toolchain.install());
  });

  it("labels are correct", () => {
    const proj = rust.project();
    expect(proj.warmup._label).toBe(":rust: warmup");
    expect(proj.test()._label).toBe(":rust: test");
    expect(proj.clippy()._label).toBe(":rust: clippy");
    expect(proj.fmt()._label).toBe(":rust: fmt");
  });

  it("with base skips apt", () => {
    const base = sh("custom base");
    const proj = rust.project({ path: "cli", base });
    const ir = pipeline([proj.test(), proj.clippy(), proj.fmt()], {
      defaultImage: "ubuntu:24.04",
    });
    const c = cmds(ir);
    expect(
      c.filter((cmd: string) => cmd.includes("apt-get install")),
    ).toHaveLength(0);
    expect(c.some((cmd: string) => cmd.includes("custom base"))).toBe(true);
  });

  it("produces valid pipeline IR", () => {
    const proj = rust.project({ path: "cli" });
    const ir = pipeline([proj.test(), proj.clippy(), proj.fmt()], {
      defaultImage: "ubuntu:24.04",
    });
    expect(ir.version).toBe("0");
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
  });

  it("toolchain escape hatch", () => {
    const proj = rust.project({ path: "cli" });
    const custom = proj.toolchain
      .install()
      .sh("custom", { label: "custom" });
    expect(custom._parent).toBe(proj.toolchain.install());
  });

  it("version forwarded", () => {
    const proj = rust.project({ path: ".", version: "1.81.0" });
    const ir = pipeline([proj.test()]);
    const rustup = stepBySubstring(ir, "sh.rustup.rs");
    expect(rustup.cmd).toContain("--default-toolchain 1.81.0");
  });
});
