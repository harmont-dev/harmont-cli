import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeEach, describe, expect, it } from "vitest";
import { clearTargetCache } from "../src/target.js";
import { pipeline } from "../src/pipeline.js";
import { sh } from "../src/step.js";
import { ttl } from "../src/cache.js";
import { go } from "../src/toolchains/go.js";
import { python } from "../src/toolchains/python.js";
import { npm } from "../src/toolchains/npm.js";
import { rust } from "../src/toolchains/rust.js";
import { zig } from "../src/toolchains/zig.js";
import { haskell } from "../src/toolchains/haskell.js";
import { cmake } from "../src/toolchains/cmake.js";

const __dir = dirname(fileURLToPath(import.meta.url));
const FIXTURES_DIR = resolve(__dir, "../../../tests/e2e/fixtures/ts");

function deepSortKeys(obj: unknown): unknown {
  if (Array.isArray(obj)) return obj.map(deepSortKeys);
  if (obj !== null && typeof obj === "object") {
    const sorted: Record<string, unknown> = {};
    for (const key of Object.keys(obj as Record<string, unknown>).sort()) {
      sorted[key] = deepSortKeys((obj as Record<string, unknown>)[key]);
    }
    return sorted;
  }
  return obj;
}

function assertFixture(name: string, ir: Record<string, unknown>): void {
  const rendered = JSON.stringify(deepSortKeys(ir), null, 2) + "\n";
  const fixturePath = resolve(FIXTURES_DIR, `${name}.json`);

  if (process.env.UPDATE_E2E_FIXTURES) {
    mkdirSync(dirname(fixturePath), { recursive: true });
    writeFileSync(fixturePath, rendered);
    return;
  }

  if (!existsSync(fixturePath)) {
    throw new Error(
      `Fixture ${fixturePath} missing — run with UPDATE_E2E_FIXTURES=1`,
    );
  }
  const expected = JSON.parse(readFileSync(fixturePath, "utf-8"));
  const actual = JSON.parse(rendered);
  expect(actual).toEqual(expected);
}

describe("E2E pipeline fixtures", () => {
  beforeEach(() => {
    clearTargetCache();
  });

  it("monorepo-ci", () => {
    const goProject = go({ path: "services/api" });
    const pyProject = python({ path: "services/ml" });
    const webProject = npm({ path: "web" });

    const ir = pipeline(
      goProject.build(),
      goProject.test(),
      goProject.vet(),
      pyProject.test(),
      pyProject.lint(),
      pyProject.typecheck(),
      webProject.run("build"),
      webProject.run("test"),
      webProject.run("lint"),
      { env: { CI: "true" }, defaultImage: "ubuntu:24.04" },
    );

    expect(ir.version).toBe("0");
    expect(ir.default_image).toBe("ubuntu:24.04");
    expect(ir.graph.nodes.length).toBeGreaterThan(0);
    assertFixture("monorepo-ci", ir);
  });

  it("rust-release", () => {
    const project = rust({ path: "." });

    const ir = pipeline(
      project.build(),
      project.test(),
      project.clippy(),
      project.fmt(),
      project.doc(),
      { env: { CI: "true" }, defaultImage: "ubuntu:24.04" },
    );

    expect(ir.version).toBe("0");
    assertFixture("rust-release", ir);
  });

  it("zig-node-polyglot", () => {
    const base = sh(
      "apt-get update && apt-get install -y --no-install-recommends " +
        "curl ca-certificates xz-utils",
      { label: ":apt: base", cache: ttl(86400), image: "ubuntu:24.04" },
    );
    const zigTc = zig({ base });
    const projA = zigTc.project("zig-a");
    const projB = zigTc.project("zig-b");
    const web = npm({ path: "web", base });

    const ir = pipeline(
      projA.build(),
      projA.test(),
      projB.build(),
      projB.test(),
      web.run("build"),
      web.run("test"),
      web.run("lint"),
      { env: { CI: "true" }, defaultImage: "ubuntu:24.04" },
    );

    expect(ir.version).toBe("0");
    assertFixture("zig-node-polyglot", ir);
  });

  it("kitchen-sink", () => {
    const hsTc = haskell({ ghc: "9.6.7" });
    const pkgA = hsTc.cabal("pkg-a");
    const pkgB = hsTc.cabal("pkg-b");
    const cProject = cmake({ path: "infra/agent", lang: "c" });

    const ir = pipeline(
      pkgA.build(),
      pkgA.test(),
      pkgB.build(),
      pkgB.test(),
      pkgB.hlint(),
      pkgB.fmt(),
      cProject.build(),
      cProject.test(),
      cProject.fmt(),
      { env: { CI: "true", STACK_ROOT: "/tmp/.stack" }, defaultImage: "ubuntu:24.04" },
    );

    expect(ir.version).toBe("0");
    assertFixture("kitchen-sink", ir);
  });
});
