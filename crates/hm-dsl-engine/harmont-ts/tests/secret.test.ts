import { describe, expect, it } from "vitest";
import { pipeline, sh, secrets, isSecretRef } from "../src/index.js";

function nodes(p: unknown) {
  return JSON.parse(JSON.stringify(p)).graph.nodes;
}

describe("secrets", () => {
  it("secrets[name] is a reference, not a value", () => {
    const ref = secrets["DEPLOY_TOKEN"];
    expect(isSecretRef(ref)).toBe(true);
    expect(ref.name).toBe("DEPLOY_TOKEN");
  });

  it("env secret refs lower into the secrets map", () => {
    const p = pipeline([sh("deploy", { env: { TOKEN: secrets["DEPLOY_TOKEN"], CI: "true" } })]);
    const n = nodes(p)[0];
    // The DSL also seeds baseline env (DEBIAN_FRONTEND, TERM) — assert our keys.
    expect(n.env.CI).toBe("true");
    expect(n.env.TOKEN).toBeUndefined();
    expect(n.secrets).toEqual({ TOKEN: "DEPLOY_TOKEN" });
    expect(n.step.secrets).toEqual({ TOKEN: "DEPLOY_TOKEN" });
  });

  it("pipeline secrets merge under step secrets", () => {
    const p = pipeline([sh("deploy", { env: { TOKEN: secrets["PIPELINE_TOKEN"] } })], {
      env: { TOKEN: secrets["GLOBAL_TOKEN"], SHARED: secrets["SHARED_SECRET"] },
    });
    const n = nodes(p)[0];
    expect(n.secrets).toEqual({ TOKEN: "PIPELINE_TOKEN", SHARED: "SHARED_SECRET" });
  });

  it("rejects invalid secret names", () => {
    expect(() => secrets["not valid!"]).toThrow(/secret name/);
  });

  it("does not fabricate refs for symbol access", () => {
    expect((secrets as Record<symbol, unknown>)[Symbol.iterator]).toBeUndefined();
  });
});
