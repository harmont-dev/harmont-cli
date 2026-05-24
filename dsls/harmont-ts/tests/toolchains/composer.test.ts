import { describe, expect, it } from "vitest";
import { composer } from "../../src/toolchains/composer.js";
import { pipeline } from "../../src/pipeline.js";

describe("composer factory", () => {
  it("returns a ComposerProject with defaults (php mode)", () => {
    const c = composer();
    expect(c.path).toBe(".");
    expect(c.install()._cmd).toContain("composer install");
  });

  it("accepts laravel flag", () => {
    const c = composer({ laravel: true });
    expect(c.test()._label).toBe(":laravel: test");
  });
});

describe("composer actions", () => {
  it("test runs phpunit by default", () => {
    expect(composer().test()._cmd).toContain("vendor/bin/phpunit");
  });

  it("test runs artisan test in laravel mode", () => {
    expect(composer({ laravel: true }).test()._cmd).toContain(
      "php artisan test",
    );
  });

  it("lint runs phpstan", () => {
    expect(composer().lint()._cmd).toContain("vendor/bin/phpstan analyse");
  });

  it("php labels use :php: prefix", () => {
    const c = composer();
    expect(c.test()._label).toBe(":php: test");
    expect(c.lint()._label).toBe(":php: lint");
  });

  it("laravel labels use :laravel: prefix", () => {
    const c = composer({ laravel: true });
    expect(c.test()._label).toBe(":laravel: test");
    expect(c.lint()._label).toBe(":laravel: lint");
  });
});

describe("composer install chain", () => {
  it("chain is: scratch → apt-base → composer-verify → deps", () => {
    const c = composer();
    const deps = c.install();
    expect(deps._label).toBe(":php: deps");

    const composerVerify = deps._parent!;
    expect(composerVerify._label).toBe(":php: composer");
  });
});

describe("composer in pipeline", () => {
  it("produces valid IR", () => {
    const c = composer();
    const ir = pipeline(c.test(), c.lint(), { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
  });
});
