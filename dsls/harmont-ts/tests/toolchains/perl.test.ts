import { describe, expect, it } from "vitest";
import { perl } from "../../src/toolchains/perl.js";
import { pipeline } from "../../src/pipeline.js";

describe("perl factory", () => {
  it("returns a PerlProject with defaults", () => {
    const p = perl();
    expect(p.path).toBe(".");
    expect(p.install()._cmd).toContain("cpanm --installdeps");
  });

  it("accepts path", () => {
    const p = perl({ path: "lib" });
    expect(p.install()._cmd).toContain("lib");
  });
});

describe("perl actions", () => {
  it("test runs prove", () => {
    expect(perl().test()._cmd).toContain("prove -lv t/");
  });

  it("lint runs perlcritic", () => {
    expect(perl().lint()._cmd).toContain("perlcritic lib/");
  });

  it("default labels use :perl: prefix", () => {
    const p = perl();
    expect(p.test()._label).toBe(":perl: test");
    expect(p.lint()._label).toBe(":perl: lint");
  });
});

describe("perl install chain", () => {
  it("chain is: scratch → apt-base → cpanm → deps", () => {
    const p = perl();
    const deps = p.install();
    expect(deps._label).toBe(":perl: deps");

    const cpanm = deps._parent!;
    expect(cpanm._label).toBe(":perl: cpanm");
  });
});

describe("perl in pipeline", () => {
  it("produces valid IR", () => {
    const p = perl();
    const ir = pipeline(p.test(), p.lint(), { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
  });
});
