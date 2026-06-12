import { describe, expect, it } from "vitest";
import { rust } from "../../src/toolchains/rust.js";

const tail = (cmd: string) =>
  cmd.slice(cmd.indexOf("cd . && ") + "cd . && ".length);

describe("rust parity golden strings", () => {
  const p = rust.project({ path: "." });
  it("matches the Python golden commands", () => {
    expect(tail(p.test({ features: ["a", "b"], nextest: true })._cmd!)).toBe(
      "cargo nextest run --workspace --features a,b --locked",
    );
    expect(tail(p.clippy({ allFeatures: true })._cmd!)).toBe(
      "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
    );
    expect(tail(p.fmt()._cmd!)).toBe("cargo fmt --all --check");
    expect(tail(p.doc({ documentPrivateItems: true })._cmd!)).toBe(
      "cargo doc --no-deps --document-private-items --workspace --locked",
    );
    expect(
      tail(p.build({ packages: ["core"], target: "wasm32-unknown-unknown" })._cmd!),
    ).toBe(
      "rustup target add wasm32-unknown-unknown && " +
        "cargo build -p core --target wasm32-unknown-unknown --locked",
    );
    expect(
      tail(p.featurePowerset({ subcommand: "check", skip: ["a b", "c"] })._cmd!),
    ).toBe("cargo hack check --feature-powerset --depth 2 --no-dev-deps --skip 'a b',c");
  });
});
