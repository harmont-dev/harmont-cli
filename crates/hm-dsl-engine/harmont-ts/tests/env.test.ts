import { afterEach, describe, expect, it } from "vitest";
import { env } from "../src/env.js";

afterEach(() => {
  delete process.env.HARMONT_TEST_ENV;
});

describe("env", () => {
  it("returns the environment value", () => {
    process.env.HARMONT_TEST_ENV = "configured";
    expect(env("HARMONT_TEST_ENV")).toBe("configured");
  });

  it("returns the default when unset", () => {
    expect(env("HARMONT_TEST_ENV", "fallback")).toBe("fallback");
  });

  it("returns undefined without a default", () => {
    expect(env("HARMONT_TEST_ENV")).toBeUndefined();
  });
});
