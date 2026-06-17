import { describe, expect, it } from "vitest";
import { pipeline } from "@harmont/hm";
import * as hm from "@harmont/hm/toolchains";

function cmdsOf(leaf: any): string[] {
  const ir = pipeline([leaf]);
  return ir.graph.nodes.map((n: any) => n.step.cmd).filter(Boolean);
}

describe("toolchain .setup()", () => {
  it("advances the install chain and is immutable", () => {
    const proj = hm.elixir({ path: "." });
    const before = proj.install();
    const advanced = proj.setup("echo __SETUP_MARKER__");
    expect(advanced).not.toBe(proj);
    expect(advanced.install()).not.toBe(before);
    const cmds = cmdsOf(advanced.install());
    expect(cmds.some((c) => c.includes("__SETUP_MARKER__"))).toBe(true);
  });

  it("is chainable", () => {
    const proj = hm.elixir({ path: "." }).setup("echo __ONE__").setup("echo __TWO__");
    const cmds = cmdsOf(proj.install());
    expect(cmds.some((c) => c.includes("__ONE__"))).toBe(true);
    expect(cmds.some((c) => c.includes("__TWO__"))).toBe(true);
  });
});

const FACTORIES: Array<[string, () => any]> = [
  ["elixir", () => hm.elixir({ path: "." })],
  ["python", () => hm.python({ path: "." })],
  ["go", () => hm.go({ path: "." })],
  ["js", () => hm.js.project({ path: "." })],
  ["zigProject", () => hm.zig({ path: "." })],
  ["zigToolchain", () => hm.zig()],
  ["rustToolchain", () => hm.rust.toolchain()],
  ["cmakeToolchain", () => hm.cmake()],
];

describe.each(FACTORIES)("%s .setup()", (_label, make) => {
  it("advances the chain (renders the setup cmd)", () => {
    const advanced = make().setup("echo __MARK__");
    const ir = pipeline([advanced.install()]);
    const cmds = ir.graph.nodes.map((n: any) => n.step.cmd).filter(Boolean);
    expect(cmds.some((c: string) => c.includes("__MARK__"))).toBe(true);
  });
});
