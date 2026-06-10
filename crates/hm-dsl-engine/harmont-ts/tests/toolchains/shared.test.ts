import { describe, expect, it } from "vitest";
import { aptBase, bunInstallCmd } from "../../src/toolchains/shared.js";
import { rust } from "../../src/toolchains/rust.js";
import { uv } from "../../src/toolchains/py/uv.js";
import { pipeline } from "../../src/pipeline.js";

describe("bunInstallCmd", () => {
  it("installs latest bun when no version given", () => {
    const cmd = bunInstallCmd();
    expect(cmd).toContain("https://bun.sh/install");
    expect(cmd).toContain("BUN_INSTALL=/usr/local");
    expect(cmd).not.toContain("bun-v");
  });

  it("installs specific version when provided", () => {
    const cmd = bunInstallCmd("1.2.0");
    expect(cmd).toContain("bun-v1.2.0");
  });
});

describe("aptBase", () => {
  it("creates a step with apt-get install", () => {
    const base = aptBase({ packages: ["curl", "ca-certificates"] });
    expect(base._cmd).toContain(
      "apt-get update && apt-get install -y curl ca-certificates",
    );
  });

  it("default label is :apt: base", () => {
    const base = aptBase({ packages: ["curl"] });
    expect(base._label).toBe(":apt: base");
  });

  it("accepts custom label", () => {
    const base = aptBase({ packages: ["curl"], label: ":lock: deps" });
    expect(base._label).toBe(":lock: deps");
  });

  it("shared across rust and python toolchains", () => {
    const base = aptBase({
      packages: [
        "curl",
        "ca-certificates",
        "build-essential",
        "pkg-config",
        "libssl-dev",
        "python3",
        "python3-venv",
      ],
    });
    const r = rust.toolchain({ base });
    const p = uv({ path: "dsls/harmont-py", base });
    const ir = pipeline([r.build(), p.test()], { defaultImage: "ubuntu:24.04" });
    const cmds = ir.graph.nodes.map(
      (n: { step: { cmd: string } }) => n.step.cmd,
    );
    const aptSteps = cmds.filter((c: string) => c.includes("apt-get install"));
    expect(aptSteps).toHaveLength(1);
  });
});
