import { describe, expect, it } from "vitest";
import { ocaml } from "../../src/toolchains/ocaml.js";
import { pipeline } from "../../src/pipeline.js";

describe("ocaml factory", () => {
  it("returns an OCamlProject with defaults", () => {
    const o = ocaml();
    expect(o.path).toBe(".");
    expect(o.install()._cmd).toContain("opam install");
  });

  it("accepts compiler version", () => {
    const o = ocaml({ compiler: "5.2.0" });
    expect(o.install()._parent!._cmd).toContain("5.2.0");
  });

  it("rejects invalid compiler", () => {
    expect(() => ocaml({ compiler: "bad" })).toThrow("invalid compiler");
  });
});

describe("ocaml actions", () => {
  it("build runs opam exec -- dune build", () => {
    expect(ocaml().build()._cmd).toContain("opam exec -- dune build");
  });

  it("test runs opam exec -- dune runtest", () => {
    expect(ocaml().test()._cmd).toContain("opam exec -- dune runtest");
  });

  it("fmt runs opam exec -- dune build @fmt", () => {
    expect(ocaml().fmt()._cmd).toContain("opam exec -- dune build @fmt");
  });

  it("default labels use :ocaml: prefix", () => {
    const o = ocaml();
    expect(o.build()._label).toBe(":ocaml: build");
    expect(o.test()._label).toBe(":ocaml: test");
    expect(o.fmt()._label).toBe(":ocaml: fmt");
  });
});

describe("ocaml install chain", () => {
  it("chain is: scratch → apt-base → opam → deps", () => {
    const o = ocaml();
    const deps = o.install();
    expect(deps._label).toBe(":ocaml: deps");

    const opam = deps._parent!;
    expect(opam._label).toBe(":ocaml: opam");
  });
});

describe("ocaml in pipeline", () => {
  it("produces valid IR", () => {
    const o = ocaml();
    const ir = pipeline(o.build(), o.test(), { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
  });
});
