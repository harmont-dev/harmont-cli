import { describe, expect, it } from "vitest";
import {
  haskell,
  HaskellToolchain,
  HaskellPackage,
} from "../../src/toolchains/haskell.js";
import { pipeline } from "../../src/pipeline.js";

describe("haskell factory", () => {
  it("returns HaskellToolchain without path", () => {
    const tc = haskell({ ghc: "9.6.7" });
    expect(tc).toBeInstanceOf(HaskellToolchain);
  });

  it("returns HaskellPackage with path", () => {
    const pkg = haskell({ ghc: "9.6.7", path: "." });
    expect(pkg).toBeInstanceOf(HaskellPackage);
    expect(pkg.path).toBe(".");
  });

  it("rejects invalid ghc version", () => {
    expect(() => haskell({ ghc: "not valid!" })).toThrow("invalid ghc");
  });
});

describe("haskell toolchain", () => {
  it("cabal creates HaskellPackage with deps step", () => {
    const tc = haskell({ ghc: "9.6.7" });
    const pkg = tc.cabal(".");
    expect(pkg).toBeInstanceOf(HaskellPackage);
    expect(pkg.install()._cmd).toContain("cabal build all --only-dependencies");
    expect(pkg.install()._label).toBe(":haskell: . deps");
  });

  it("multiple packages share ghcup install", () => {
    const tc = haskell({ ghc: "9.6.7" });
    const a = tc.cabal("pkg-a");
    const b = tc.cabal("pkg-b");
    expect(a.install()._parent).toBe(b.install()._parent);
  });
});

describe("haskell package actions", () => {
  it("build runs cabal build all", () => {
    const pkg = haskell({ ghc: "9.6.7", path: "." });
    expect(pkg.build()._cmd).toContain("cabal build all");
  });

  it("test runs cabal test all", () => {
    const pkg = haskell({ ghc: "9.6.7", path: "." });
    expect(pkg.test()._cmd).toContain("cabal test all");
  });

  it("lint runs cabal build all --flag werror", () => {
    const pkg = haskell({ ghc: "9.6.7", path: "." });
    expect(pkg.lint()._cmd).toContain("--flag werror");
  });

  it("hlint runs hlint on path", () => {
    const pkg = haskell({ ghc: "9.6.7", path: "src" });
    expect(pkg.hlint()._cmd).toContain("hlint src");
  });

  it("fmt runs fourmolu on path", () => {
    const pkg = haskell({ ghc: "9.6.7", path: "." });
    expect(pkg.fmt()._cmd).toContain("fourmolu --mode check .");
  });

  it("labels include path", () => {
    const pkg = haskell({ ghc: "9.6.7", path: "my-pkg" });
    expect(pkg.build()._label).toBe(":haskell: my-pkg build");
    expect(pkg.test()._label).toBe(":haskell: my-pkg test");
  });
});

describe("haskell install chain", () => {
  it("chain is: scratch → apt-base → ghcup", () => {
    const tc = haskell({ ghc: "9.6.7" });
    const install = tc.install();
    expect(install._label).toBe(":haskell: ghcup");
    expect(install._cmd).toContain("ghcup install ghc 9.6.7");
  });
});

describe("haskell in pipeline", () => {
  it("produces valid IR", () => {
    const pkg = haskell({ ghc: "9.6.7", path: "." });
    const ir = pipeline(pkg.build(), pkg.test(), pkg.fmt(), {
      defaultImage: "ubuntu:24.04",
    });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(5);
    expect(ir.version).toBe("0");
  });
});
