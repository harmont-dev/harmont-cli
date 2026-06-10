import { readdirSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeEach, describe, expect, it } from "vitest";
import { clearTargetCache } from "../src/target.js";

const __dir = dirname(fileURLToPath(import.meta.url));
const EXAMPLES_ROOT = resolve(__dir, "../../../../examples");

function exampleDirs(): string[] {
  if (!existsSync(EXAMPLES_ROOT)) return [];
  return readdirSync(EXAMPLES_ROOT, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .filter((d) =>
      existsSync(join(EXAMPLES_ROOT, d.name, ".hm", "pipeline.ts")),
    )
    .map((d) => d.name)
    .sort();
}

const examples = exampleDirs();

describe.skipIf(examples.length === 0)("examples render to v0 IR", () => {
  beforeEach(() => {
    clearTargetCache();
  });

  for (const name of examples) {
    it(`${name}: produces valid CI pipeline IR`, async () => {
      const pipelinePath = join(
        EXAMPLES_ROOT,
        name,
        ".hm",
        "pipeline.ts",
      );
      const mod = await import(pipelinePath);
      const definitions = mod.default ?? mod.pipelines;

      expect(Array.isArray(definitions)).toBe(true);
      expect(definitions.length).toBeGreaterThan(0);

      const ci = definitions.find((d: any) => d.slug === "ci");
      expect(ci).toBeDefined();
      expect(ci.pipeline.version).toBe("0");
      expect(ci.pipeline.graph.nodes.length).toBeGreaterThan(0);
      expect(ci.pipeline.graph.edge_property).toBe("directed");
      expect(ci.pipeline.default_image).toBeTruthy();

      // Verify all nodes have required fields
      for (const node of ci.pipeline.graph.nodes) {
        expect(node.step.key).toBeDefined();
        expect(node.step.cmd).toBeDefined();
        expect(typeof node.env).toBe("object");
      }

      // Verify edges reference valid node indices
      for (const [src, dst, kind] of ci.pipeline.graph.edges) {
        expect(src).toBeLessThan(ci.pipeline.graph.nodes.length);
        expect(dst).toBeLessThan(ci.pipeline.graph.nodes.length);
        expect(["builds_in", "depends_on"]).toContain(kind);
      }
    });
  }

});
