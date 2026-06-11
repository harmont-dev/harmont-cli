import { describe, expect, it } from "vitest";
import { elixir } from "../../src/toolchains/elixir.js";
import { sh, timeout } from "../../src/step.js";
import { pipeline } from "../../src/pipeline.js";

describe("elixir factory", () => {
  it("returns an ElixirProject with defaults", () => {
    const ex = elixir();
    expect(ex.path).toBe(".");
    expect(ex.install()._cmd).toContain("mix deps.get");
  });

  it("accepts path and versions", () => {
    const ex = elixir({ path: "apps/api", elixirVersion: "1.18.3", otpVersion: "27.3.3" });
    expect(ex.path).toBe("apps/api");
    expect(ex.install()._parent!._cmd).toContain("1.18.3");
  });

  it("rejects invalid elixir version", () => {
    expect(() => elixir({ elixirVersion: "abc" })).toThrow("invalid elixir version");
  });

  it("rejects invalid otp version", () => {
    expect(() => elixir({ otpVersion: "xyz" })).toThrow("invalid otp version");
  });
});

describe("elixir actions", () => {
  it("compile runs mix compile --warnings-as-errors", () => {
    expect(elixir().compile()._cmd).toContain("mix compile --warnings-as-errors");
  });

  it("test runs mix test", () => {
    expect(elixir().test()._cmd).toContain("mix test");
  });

  it("format runs mix format --check-formatted", () => {
    expect(elixir().format()._cmd).toContain("mix format --check-formatted");
  });

  it("credo runs mix credo --strict", () => {
    expect(elixir().credo()._cmd).toContain("mix credo --strict");
  });

  it("plt builds PLT with onChange cache", () => {
    const ex = elixir();
    const step = ex.plt();
    expect(step._cmd).toContain("mix dialyzer --plt");
    expect(step._label).toBe(":ex: plt");
    expect(step._cache).toEqual({ kind: "on_change", paths: ["./mix.lock"] });
  });

  it("dialyzer chains through plt step", () => {
    const ex = elixir();
    const step = ex.dialyzer();
    expect(step._cmd).toContain("mix dialyzer");
    expect(step._cmd).not.toContain("--plt");
    expect(step._parent!._label).toBe(":ex: plt");
  });

  it("plt is memoized per project instance", () => {
    const ex = elixir();
    expect(ex.plt()).toBe(ex.plt());
  });

  it("sobelow runs mix sobelow --exit", () => {
    expect(elixir().sobelow()._cmd).toContain("mix sobelow --exit");
  });

  it("depsAudit runs mix deps.audit", () => {
    expect(elixir().depsAudit()._cmd).toContain("mix deps.audit");
  });

  it("hexAudit runs mix hex.audit", () => {
    expect(elixir().hexAudit()._cmd).toContain("mix hex.audit");
  });

  it("release runs MIX_ENV=prod mix release", () => {
    const step = elixir().release();
    expect(step._cmd).toContain("MIX_ENV=prod mix release");
  });

  it("test with cover flag", () => {
    expect(elixir().test({ cover: true })._cmd).toContain("--cover");
  });

  it("test with partitions flag", () => {
    expect(elixir().test({ partitions: 4 })._cmd).toContain("--partitions 4");
  });

  it("release with custom env", () => {
    expect(elixir().release({ mixEnv: "staging" })._cmd).toContain("MIX_ENV=staging");
  });

  it("actions chain from install step", () => {
    const ex = elixir();
    expect(ex.compile()._parent).toBe(ex.install());
  });

  it("accepts step options", () => {
    const ex = elixir();
    const t = timeout(600, ex.test({ label: "my test" }));
    expect(t._label).toBe("my test");
    expect(t._timeoutSeconds).toBe(600);
  });

  it("default labels use :ex: prefix", () => {
    const ex = elixir();
    expect(ex.compile()._label).toBe(":ex: compile");
    expect(ex.test()._label).toBe(":ex: test");
    expect(ex.format()._label).toBe(":ex: format");
    expect(ex.credo()._label).toBe(":ex: credo");
    expect(ex.dialyzer()._label).toBe(":ex: dialyzer");
  });
});

describe("elixir install chain", () => {
  it("chain is: scratch → apt-base → erlang → elixir → mix-deps", () => {
    const ex = elixir();
    const deps = ex.install();
    expect(deps._label).toBe(":ex: mix-deps");

    const elixirInstall = deps._parent!;
    expect(elixirInstall._label).toBe(":ex: elixir-install");

    const erlangInstall = elixirInstall._parent!;
    expect(erlangInstall._label).toBe(":ex: erlang-install");
  });

  it("accepts base step", () => {
    const base = sh("custom base");
    const ex = elixir({ base });
    // chain: base → erlang-install → elixir-install → mix-deps
    const elixirInstall = ex.install()._parent!;
    const erlangInstall = elixirInstall._parent!;
    expect(erlangInstall._parent).toBe(base);
  });

  it("accepts custom image", () => {
    const ex = elixir({ image: "debian:12" });
    const deps = ex.install();
    const elixirStep = deps._parent!;
    const erlangStep = elixirStep._parent!;
    const aptBase = erlangStep._parent!;
    const root = aptBase._parent!;
    expect(root._image).toBe("debian:12");
  });
});

describe("elixir in pipeline", () => {
  it("produces valid IR", () => {
    const ex = elixir();
    const ir = pipeline([ex.compile(), ex.test(), ex.format()]);
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(5);
    expect(ir.version).toBe("0");
  });
});
