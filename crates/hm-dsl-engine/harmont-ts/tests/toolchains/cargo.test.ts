import { describe, expect, it } from "vitest";
import { cargoFlags, shQuote } from "../../src/toolchains/cargo.js";

describe("shQuote", () => {
  it("leaves simple identifiers alone", () => {
    expect(shQuote("harmont-core")).toBe("harmont-core");
  });
  it("quotes values with shell metacharacters", () => {
    expect(shQuote("a; rm -rf /")).toBe("'a; rm -rf /'");
  });
  it("escapes embedded single quotes like shlex.quote", () => {
    expect(shQuote("a'b")).toBe("'a'\"'\"'b'");
  });
  it("quotes the empty string", () => {
    expect(shQuote("")).toBe("''");
  });
});

describe("cargoFlags", () => {
  it("emits only --locked for empty opts", () => {
    expect(cargoFlags({})).toBe(" --locked");
  });
  it("locked can be disabled", () => {
    expect(cargoFlags({ locked: false })).toBe("");
  });
  it("workspace scope", () => {
    expect(cargoFlags({ workspace: true })).toBe(" --workspace --locked");
  });
  it("packages take precedence and quote values", () => {
    expect(cargoFlags({ workspace: true, packages: ["a", "b c"] })).toBe(
      " -p a -p 'b c' --locked",
    );
  });
  it("exclude pairs with workspace", () => {
    expect(cargoFlags({ workspace: true, exclude: ["b"] })).toBe(
      " --workspace --exclude b --locked",
    );
  });
  it("exclude without workspace throws", () => {
    expect(() => cargoFlags({ exclude: ["b"] })).toThrow("workspace");
  });
  it("exclude with packages throws", () => {
    expect(() => cargoFlags({ packages: ["a"], exclude: ["b"] })).toThrow("exclude");
  });
  it("all-features", () => {
    expect(cargoFlags({ allFeatures: true })).toBe(" --all-features --locked");
  });
  it("features joined comma", () => {
    expect(cargoFlags({ features: ["x", "y"] })).toBe(" --features x,y --locked");
  });
  it("no-default-features with features", () => {
    expect(cargoFlags({ noDefaultFeatures: true, features: ["x"] })).toBe(
      " --no-default-features --features x --locked",
    );
  });
  it("full token order", () => {
    expect(
      cargoFlags({
        packages: ["core"],
        allTargets: true,
        noDefaultFeatures: true,
        features: ["a", "b"],
        target: "x86_64-unknown-linux-gnu",
        profile: "ci",
        flags: ["--keep-going"],
      }),
    ).toBe(
      " -p core --all-targets --no-default-features --features a,b" +
        " --target x86_64-unknown-linux-gnu --profile ci --locked --keep-going",
    );
  });
  it("flags are verbatim", () => {
    expect(cargoFlags({ locked: false, flags: ["--features=a b"] })).toBe(
      " --features=a b",
    );
  });
  it("throws on all-features + features conflict", () => {
    expect(() => cargoFlags({ allFeatures: true, features: ["x"] })).toThrow(
      "all-features",
    );
  });
  it("throws on release + profile conflict", () => {
    expect(() => cargoFlags({ release: true, profile: "ci" })).toThrow("profile");
  });
});
