import { describe, expect, it } from "vitest";
import { ruby } from "../../src/toolchains/ruby.js";
import { pipeline } from "../../src/pipeline.js";

describe("ruby factory", () => {
  it("returns a RubyProject with defaults", () => {
    const r = ruby();
    expect(r.path).toBe(".");
    expect(r.install()._cmd).toContain("bundle install");
  });

  it("accepts path", () => {
    const r = ruby({ path: "apps/web" });
    expect(r.install()._cmd).toContain("apps/web");
  });

  it("rejects invalid version", () => {
    expect(() => ruby({ version: "abc" })).toThrow("invalid version");
  });

  it("rejects pinned version (not implemented)", () => {
    expect(() => ruby({ version: "3.3" })).toThrow("not yet implemented");
  });
});

describe("ruby actions", () => {
  it("test runs bundle exec rspec", () => {
    expect(ruby().test()._cmd).toContain("bundle exec rspec");
  });

  it("lint runs bundle exec rubocop", () => {
    expect(ruby().lint()._cmd).toContain("bundle exec rubocop");
  });

  it("default labels use :ruby: prefix", () => {
    const r = ruby();
    expect(r.test()._label).toBe(":ruby: test");
    expect(r.lint()._label).toBe(":ruby: lint");
  });
});

describe("ruby install chain", () => {
  it("chain is: scratch → apt-base → bundler → deps", () => {
    const r = ruby();
    const deps = r.install();
    expect(deps._label).toBe(":ruby: deps");

    const bundler = deps._parent!;
    expect(bundler._label).toBe(":ruby: bundler");
  });
});

describe("ruby in pipeline", () => {
  it("produces valid IR", () => {
    const r = ruby();
    const ir = pipeline([r.test(), r.lint()], { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
  });
});
