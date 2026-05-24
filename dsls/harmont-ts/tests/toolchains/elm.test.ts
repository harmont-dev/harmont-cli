import { describe, expect, it } from "vitest";
import { elm } from "../../src/toolchains/elm.js";
import { pipeline } from "../../src/pipeline.js";

describe("elm factory", () => {
  it("returns an ElmProject with defaults", () => {
    const e = elm();
    expect(e.path).toBe(".");
    expect(e.install()._cmd).toContain("/usr/local/bin/elm");
  });

  it("accepts elm and node versions", () => {
    const e = elm({ elmVersion: "0.19.1", nodeVersion: "22" });
    expect(e.install()._cmd).toContain("0.19.1");
    expect(e.install()._parent!._cmd).toContain("setup_22");
  });

  it("rejects invalid elm version", () => {
    expect(() => elm({ elmVersion: "abc" })).toThrow("invalid elm version");
  });

  it("rejects invalid node version", () => {
    expect(() => elm({ nodeVersion: "abc" })).toThrow("invalid node version");
  });
});

describe("elm actions", () => {
  it("make compiles target", () => {
    expect(elm().make("src/Main.elm")._cmd).toContain("elm make src/Main.elm");
  });

  it("make accepts output flag", () => {
    expect(elm().make("src/Main.elm", { output: "app.js" })._cmd).toContain(
      "--output=app.js",
    );
  });

  it("test runs elm-test via npx", () => {
    expect(elm().test()._cmd).toContain("npx --yes elm-test");
  });

  it("review runs elm-review via npx", () => {
    expect(elm().review()._cmd).toContain("npx --yes elm-review");
  });

  it("fmt runs elm-format via npx", () => {
    expect(elm().fmt()._cmd).toContain("npx --yes elm-format --validate");
  });

  it("default labels use :elm: prefix", () => {
    const e = elm();
    expect(e.test()._label).toBe(":elm: test");
    expect(e.review()._label).toBe(":elm: review");
    expect(e.fmt()._label).toBe(":elm: fmt");
  });
});

describe("elm install chain", () => {
  it("chain is: scratch → apt-base → node → elm-binary", () => {
    const e = elm();
    const elmInstall = e.install();
    expect(elmInstall._label).toBe(":elm: install");

    const nodeInstall = elmInstall._parent!;
    expect(nodeInstall._label).toBe(":elm: node");

    const aptBase = nodeInstall._parent!;
    expect(aptBase._cmd).toContain("apt-get");
  });
});

describe("elm in pipeline", () => {
  it("produces valid IR", () => {
    const e = elm();
    const ir = pipeline(e.test(), e.fmt(), { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
  });
});
