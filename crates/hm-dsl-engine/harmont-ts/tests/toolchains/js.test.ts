import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { js, ts, JsProject } from "../../src/toolchains/js.js";
import { sh, timeout } from "../../src/step.js";
import { pipeline } from "../../src/pipeline.js";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

// ---------------------------------------------------------------------------
// Factory defaults
// ---------------------------------------------------------------------------

describe("js.project factory defaults", () => {
  it("returns JsProject with defaults (node + npm), path '.'", () => {
    const p = js.project();
    expect(p).toBeInstanceOf(JsProject);
    expect(p.path).toBe(".");
    expect(p.install()._cmd).toContain("npm ci");
  });

  it("accepts path option", () => {
    const p = js.project({ path: "packages/app" });
    expect(p.path).toBe("packages/app");
    expect(p.install()._cmd).toContain("packages/app");
  });

  it("accepts node version", () => {
    const p = js.project({ version: "22" });
    expect(p.install()._parent!._cmd).toContain("setup_22");
  });

  it("defaults pm to bun when runtime=bun", () => {
    const p = js.project({ runtime: "bun" });
    expect(p.install()._cmd).toContain("bun install");
  });
});

// ---------------------------------------------------------------------------
// Version validation
// ---------------------------------------------------------------------------

describe("js.project version validation", () => {
  it("rejects invalid node version", () => {
    expect(() => js.project({ version: "abc" })).toThrow("invalid version");
  });

  it("accepts node version with .x suffix", () => {
    expect(() => js.project({ version: "22.x" })).not.toThrow();
  });

  it("rejects invalid bun version", () => {
    expect(() => js.project({ runtime: "bun", version: "abc" })).toThrow(
      "invalid version",
    );
  });

  it("accepts bun two-part semver", () => {
    expect(() => js.project({ runtime: "bun", version: "1.2" })).not.toThrow();
  });

  it("accepts bun three-part semver", () => {
    expect(() =>
      js.project({ runtime: "bun", version: "1.2.3" }),
    ).not.toThrow();
  });

  it("rejects invalid deno version", () => {
    expect(() => js.project({ runtime: "deno", version: "abc" })).toThrow(
      "invalid version",
    );
  });

  it("accepts deno semver", () => {
    expect(() =>
      js.project({ runtime: "deno", version: "2.0.0" }),
    ).not.toThrow();
  });
});

// ---------------------------------------------------------------------------
// PM / runtime validation
// ---------------------------------------------------------------------------

describe("js.project PM/runtime validation", () => {
  it("rejects pm=npm with runtime=bun", () => {
    expect(() => js.project({ pm: "npm", runtime: "bun" })).toThrow(
      'runtime="bun" only supports pm="bun"',
    );
  });

  it("rejects pm=pnpm with runtime=bun", () => {
    expect(() => js.project({ pm: "pnpm", runtime: "bun" })).toThrow(
      'runtime="bun" only supports pm="bun"',
    );
  });

  it("rejects pm option with runtime=deno", () => {
    expect(() => js.project({ pm: "npm", runtime: "deno" })).toThrow(
      "do not set pm",
    );
  });

  it("allows pm=bun with runtime=node", () => {
    expect(() => js.project({ pm: "bun", runtime: "node" })).not.toThrow();
  });

  it("allows pm=bun with runtime=bun", () => {
    expect(() => js.project({ pm: "bun", runtime: "bun" })).not.toThrow();
  });

  it("allows yarn-classic / yarn-berry with runtime=node", () => {
    expect(() => js.project({ pm: "yarn-classic" })).not.toThrow();
    expect(() => js.project({ pm: "yarn-berry" })).not.toThrow();
  });

  it("rejects yarn with runtime=bun", () => {
    expect(() => js.project({ pm: "yarn-berry", runtime: "bun" })).toThrow(
      'runtime="bun" only supports pm="bun"',
    );
  });

  it("rejects pm=deno with non-deno runtime", () => {
    expect(() => js.project({ pm: "deno", runtime: "node" })).toThrow(
      'pm="deno" is not valid',
    );
    expect(() => js.project({ pm: "deno", runtime: "bun" })).toThrow(
      'pm="deno" is not valid',
    );
  });
});

// ---------------------------------------------------------------------------
// Install chain structure — node + npm
// ---------------------------------------------------------------------------

describe("js install chain: node + npm", () => {
  it("chain is: scratch → apt-base → node-install → npm-ci", () => {
    const p = js.project();
    const npmCi = p.install();
    expect(npmCi._cmd).toContain("npm ci");

    const nodeInstall = npmCi._parent!;
    expect(nodeInstall._cmd).toContain("nodejs");
    expect(nodeInstall._cache).toBeDefined();

    const aptBase = nodeInstall._parent!;
    expect(aptBase._cmd).toContain("apt-get");

    const root = aptBase._parent!;
    expect(root._cmd).toBeNull(); // scratch
  });
});

// ---------------------------------------------------------------------------
// Install chain structure — node + pnpm
// ---------------------------------------------------------------------------

describe("js install chain: node + pnpm", () => {
  it("chain is: scratch → apt-base → node-install → corepack-pnpm → pnpm-deps", () => {
    const p = js.project({ pm: "pnpm" });
    const pnpmDeps = p.install();
    expect(pnpmDeps._cmd).toContain("pnpm install --frozen-lockfile");

    const corepack = pnpmDeps._parent!;
    expect(corepack._cmd).toContain("corepack enable pnpm");

    const nodeInstall = corepack._parent!;
    expect(nodeInstall._cmd).toContain("nodejs");

    const aptBase = nodeInstall._parent!;
    expect(aptBase._cmd).toContain("apt-get");

    const root = aptBase._parent!;
    expect(root._cmd).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Install chain structure — node + yarn (classic + berry)
// ---------------------------------------------------------------------------

describe("js install chain: node + yarn", () => {
  it("yarn-classic: corepack enable → yarn install --frozen-lockfile", () => {
    const p = js.project({ pm: "yarn-classic" });
    const deps = p.install();
    expect(deps._cmd).toContain("yarn install --frozen-lockfile");
    expect(deps._cache).toBeDefined();

    const corepack = deps._parent!;
    expect(corepack._cmd).toContain("corepack enable");

    const nodeInstall = corepack._parent!;
    expect(nodeInstall._cmd).toContain("nodejs");
  });

  it("yarn-berry: corepack enable → yarn install --immutable", () => {
    const p = js.project({ pm: "yarn-berry" });
    const deps = p.install();
    expect(deps._cmd).toContain("yarn install --immutable");

    const corepack = deps._parent!;
    expect(corepack._cmd).toContain("corepack enable");
  });

  it("both yarn variants watch yarn.lock", () => {
    for (const pm of ["yarn-classic", "yarn-berry"] as const) {
      const deps = js.project({ pm }).install();
      const cache = deps._cache as { paths?: string[] };
      expect(JSON.stringify(cache)).toContain("yarn.lock");
    }
  });
});

// ---------------------------------------------------------------------------
// Install chain structure — bun + bun
// ---------------------------------------------------------------------------

describe("js install chain: bun + bun", () => {
  it("chain is: scratch → apt-base(+unzip) → bun-install → bun-deps", () => {
    const p = js.project({ runtime: "bun" });
    const bunDeps = p.install();
    expect(bunDeps._cmd).toContain("bun install --frozen-lockfile");

    const bunSetup = bunDeps._parent!;
    expect(bunSetup._cmd).toContain("bun.sh/install");
    expect(bunSetup._cache).toBeDefined();

    const aptBase = bunSetup._parent!;
    expect(aptBase._cmd).toContain("apt-get");
    expect(aptBase._cmd).toContain("unzip");

    const root = aptBase._parent!;
    expect(root._cmd).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Install chain structure — node + bun (PM)
// ---------------------------------------------------------------------------

describe("js install chain: node + bun (as PM)", () => {
  it("chain is: scratch → apt-base(+unzip) → node-install → bun-pm-install → bun-deps", () => {
    const p = js.project({ runtime: "node", pm: "bun" });
    const bunDeps = p.install();
    expect(bunDeps._cmd).toContain("bun install --frozen-lockfile");

    const bunPmInstall = bunDeps._parent!;
    expect(bunPmInstall._cmd).toContain("bun.sh/install");

    const nodeInstall = bunPmInstall._parent!;
    expect(nodeInstall._cmd).toContain("nodejs");

    const aptBase = nodeInstall._parent!;
    expect(aptBase._cmd).toContain("apt-get");
    expect(aptBase._cmd).toContain("unzip");

    const root = aptBase._parent!;
    expect(root._cmd).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Install chain structure — deno
// ---------------------------------------------------------------------------

describe("js install chain: deno", () => {
  it("chain is: scratch → apt-base(+unzip) → deno-install → deno-deps", () => {
    const p = js.project({ runtime: "deno" });
    const denoDeps = p.install();
    expect(denoDeps._cmd).toContain("deno install");

    const denoSetup = denoDeps._parent!;
    expect(denoSetup._cmd).toContain("deno.land/install.sh");
    expect(denoSetup._cache).toBeDefined();

    const aptBase = denoSetup._parent!;
    expect(aptBase._cmd).toContain("apt-get");
    expect(aptBase._cmd).toContain("unzip");

    const root = aptBase._parent!;
    expect(root._cmd).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Install chain — base step and custom image
// ---------------------------------------------------------------------------

describe("js install chain: base step and custom image", () => {
  it("accepts base step", () => {
    const customBase = sh("custom base");
    const p = js.project({ base: customBase });
    const npmCi = p.install();
    const nodeInstall = npmCi._parent!;
    expect(nodeInstall._parent).toBe(customBase);
  });

  it("accepts custom image", () => {
    const p = js.project({ image: "debian:12" });
    const npmCi = p.install();
    const nodeInstall = npmCi._parent!;
    const aptBase = nodeInstall._parent!;
    const root = aptBase._parent!;
    expect(root._image).toBe("debian:12");
  });
});

// ---------------------------------------------------------------------------
// Actions — uniform run() across all PMs/runtimes
// ---------------------------------------------------------------------------

describe("js.project actions", () => {
  it("run() executes arbitrary script via npm run", () => {
    const p = js.project();
    const r = p.run("typecheck");
    expect(r._cmd).toContain("npm run typecheck");
  });

  it("run() uses pnpm run for pnpm PM", () => {
    const p = js.project({ pm: "pnpm" });
    expect(p.run("typecheck")._cmd).toContain("pnpm run typecheck");
  });

  it("run() uses yarn run for yarn PMs", () => {
    expect(js.project({ pm: "yarn-classic" }).run("test")._cmd).toContain(
      "yarn run test",
    );
    expect(js.project({ pm: "yarn-berry" }).run("test")._cmd).toContain(
      "yarn run test",
    );
  });

  it("run() uses bun run for bun runtime", () => {
    const p = js.project({ runtime: "bun" });
    expect(p.run("typecheck")._cmd).toContain("bun run typecheck");
  });

  it("run() uses deno task for deno runtime", () => {
    const p = js.project({ runtime: "deno" });
    expect(p.run("typecheck")._cmd).toContain("deno task typecheck");
  });

  it("actions attach to install (fan-out)", () => {
    const p = js.project();
    expect(p.run("test")._parent).toBe(p.install());
    expect(p.run("lint")._parent).toBe(p.install());
  });

  it("actions respect custom path", () => {
    const p = js.project({ path: "packages/ui" });
    expect(p.run("test")._cmd).toContain("cd packages/ui");
  });

  it("actions accept step options (label, timeoutSeconds)", () => {
    const p = js.project();
    const t = timeout(300, p.run("test", { label: "my test" }));
    expect(t._label).toBe("my test");
    expect(t._timeoutSeconds).toBe(300);
  });

  it("default label is :<tag>: <script>", () => {
    expect(js.project().run("test")._label).toBe(":node: test");
    expect(js.project({ runtime: "bun" }).run("test")._label).toBe(":bun: test");
    expect(js.project({ runtime: "deno" }).run("fmt")._label).toBe(":deno: fmt");
  });
});

// ---------------------------------------------------------------------------
// Pipeline IR
// ---------------------------------------------------------------------------

describe("js.project pipeline IR", () => {
  it("produces valid IR for node+npm", () => {
    const p = js.project();
    const ir = pipeline([p.run("test"), p.run("lint")], {
      defaultImage: "ubuntu:24.04",
    });
    expect(ir.version).toBe("0");
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
  });

  it("produces valid IR for node+pnpm", () => {
    const p = js.project({ pm: "pnpm" });
    const ir = pipeline([p.run("test"), p.run("lint")], {
      defaultImage: "ubuntu:24.04",
    });
    expect(ir.version).toBe("0");
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(5);
  });

  it("produces valid IR for node+yarn-berry", () => {
    const p = js.project({ pm: "yarn-berry" });
    const ir = pipeline([p.run("test"), p.run("lint")], {
      defaultImage: "ubuntu:24.04",
    });
    expect(ir.version).toBe("0");
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(5);
  });

  it("produces valid IR for bun+bun", () => {
    const p = js.project({ runtime: "bun" });
    const ir = pipeline([p.run("test"), p.run("lint")], {
      defaultImage: "ubuntu:24.04",
    });
    expect(ir.version).toBe("0");
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
  });

  it("produces valid IR for deno", () => {
    const p = js.project({ runtime: "deno" });
    const ir = pipeline([p.run("test"), p.run("lint")], {
      defaultImage: "ubuntu:24.04",
    });
    expect(ir.version).toBe("0");
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
  });

  it("test and lint share install as parent (fan-out)", () => {
    const p = js.project();
    const t = p.run("test");
    const l = p.run("lint");
    expect(t._parent).toBe(p.install());
    expect(l._parent).toBe(p.install());
  });
});

// ---------------------------------------------------------------------------
// Auto-detection
// ---------------------------------------------------------------------------

describe("js.project auto-detection", () => {
  let tmp: string;

  beforeEach(() => {
    tmp = mkdtempSync(join(tmpdir(), "hm-js-detect-"));
    writeFileSync(join(tmp, "package.json"), "{}");
  });

  afterEach(() => {
    rmSync(tmp, { recursive: true, force: true });
  });

  it("detects pnpm from pnpm-lock.yaml", () => {
    writeFileSync(join(tmp, "pnpm-lock.yaml"), "");
    const p = js.project({ path: tmp });
    expect(p.install()._cmd).toContain("pnpm install --frozen-lockfile");
  });

  it("detects bun runtime + pm from bun.lock", () => {
    writeFileSync(join(tmp, "bun.lock"), "");
    const p = js.project({ path: tmp });
    expect(p.install()._cmd).toContain("bun install --frozen-lockfile");
    const bunSetup = p.install()._parent!;
    expect(bunSetup._cmd).toContain("bun.sh/install");
  });

  it("detects bun from engines.bun in package.json", () => {
    writeFileSync(
      join(tmp, "package.json"),
      JSON.stringify({ engines: { bun: ">=1.0" } }),
    );
    const p = js.project({ path: tmp });
    expect(p.install()._cmd).toContain("bun install --frozen-lockfile");
    const bunSetup = p.install()._parent!;
    expect(bunSetup._cmd).toContain("bun.sh/install");
  });

  it("detects deno from deno.lock", () => {
    writeFileSync(join(tmp, "deno.lock"), "");
    const p = js.project({ path: tmp });
    expect(p.install()._cmd).toContain("deno install");
  });

  it("detects pnpm from packageManager field", () => {
    writeFileSync(
      join(tmp, "package.json"),
      JSON.stringify({ packageManager: "pnpm@8.15.4" }),
    );
    const p = js.project({ path: tmp });
    expect(p.install()._cmd).toContain("pnpm install --frozen-lockfile");
  });

  it("explicit opts skip detection entirely", () => {
    writeFileSync(join(tmp, "bun.lock"), "");
    const p = js.project({ path: tmp, pm: "npm", runtime: "node" });
    expect(p.install()._cmd).toContain("npm ci");
  });

  it("defaults to node + npm when no detection signals", () => {
    const p = js.project({ path: tmp });
    expect(p.install()._cmd).toContain("npm ci");
  });

  it("skips detection when only runtime is set", () => {
    writeFileSync(join(tmp, "pnpm-lock.yaml"), "");
    const p = js.project({ path: tmp, runtime: "node" });
    expect(p.install()._cmd).toContain("npm ci");
  });

  it("skips detection when only pm is set", () => {
    writeFileSync(join(tmp, "bun.lock"), "");
    const p = js.project({ path: tmp, pm: "pnpm" });
    expect(p.install()._cmd).toContain("pnpm install --frozen-lockfile");
  });

  it("detects yarn-berry from packageManager field", () => {
    writeFileSync(
      join(tmp, "package.json"),
      JSON.stringify({ packageManager: "yarn@4.5.0" }),
    );
    writeFileSync(join(tmp, "yarn.lock"), "");
    const p = js.project({ path: tmp });
    expect(p.install()._cmd).toContain("yarn install --immutable");
  });

  it("detects yarn-classic from yarn.lock alone", () => {
    writeFileSync(join(tmp, "yarn.lock"), "");
    const p = js.project({ path: tmp });
    expect(p.install()._cmd).toContain("yarn install --frozen-lockfile");
  });
});

// ---------------------------------------------------------------------------
// Install chain — corepack version pinning
// ---------------------------------------------------------------------------

describe("js install chain: corepack version pinning", () => {
  let tmp: string;

  beforeEach(() => {
    tmp = mkdtempSync(join(tmpdir(), "hm-js-corepack-"));
  });

  afterEach(() => {
    rmSync(tmp, { recursive: true, force: true });
  });

  it("corepack command includes version from packageManager field", () => {
    writeFileSync(
      join(tmp, "package.json"),
      JSON.stringify({ packageManager: "pnpm@10.33.0" }),
    );
    writeFileSync(join(tmp, "pnpm-lock.yaml"), "");
    const p = js.project({ path: tmp });
    const deps = p.install();
    const corepack = deps._parent!;
    expect(corepack._cmd).toBe("corepack enable pnpm && corepack install -g pnpm@10.33.0");
  });

  it("corepack command includes version for yarn-berry", () => {
    writeFileSync(
      join(tmp, "package.json"),
      JSON.stringify({ packageManager: "yarn@4.5.0" }),
    );
    writeFileSync(join(tmp, "yarn.lock"), "");
    const p = js.project({ path: tmp });
    const deps = p.install();
    const corepack = deps._parent!;
    expect(corepack._cmd).toBe("corepack enable yarn && corepack install -g yarn@4.5.0");
  });

  it("corepack command has no version when packageManager field absent", () => {
    writeFileSync(join(tmp, "package.json"), "{}");
    writeFileSync(join(tmp, "pnpm-lock.yaml"), "");
    const p = js.project({ path: tmp });
    const deps = p.install();
    const corepack = deps._parent!;
    expect(corepack._cmd).toBe("corepack enable pnpm");
  });

  it("explicit pm option without packageManager field omits version", () => {
    const p = js.project({ pm: "pnpm" });
    const deps = p.install();
    const corepack = deps._parent!;
    expect(corepack._cmd).toBe("corepack enable pnpm");
  });

  it("corepack step cache watches package.json for version changes", () => {
    writeFileSync(
      join(tmp, "package.json"),
      JSON.stringify({ packageManager: "pnpm@10.33.0" }),
    );
    writeFileSync(join(tmp, "pnpm-lock.yaml"), "");
    const p = js.project({ path: tmp });
    const deps = p.install();
    const corepack = deps._parent!;
    expect(corepack._cache).toEqual({
      kind: "on_change",
      paths: [`${tmp}/package.json`],
    });
  });

  it("corepack step cache is forever when no packageManager field", () => {
    writeFileSync(join(tmp, "package.json"), "{}");
    writeFileSync(join(tmp, "pnpm-lock.yaml"), "");
    const p = js.project({ path: tmp });
    const deps = p.install();
    const corepack = deps._parent!;
    expect(corepack._cache).toEqual({ kind: "forever", envKeys: [] });
  });
});

// ---------------------------------------------------------------------------
// ts alias
// ---------------------------------------------------------------------------

describe("ts alias", () => {
  it("ts is same object as js", () => {
    expect(ts).toBe(js);
  });

  it("ts.project returns JsProject", () => {
    const p = ts.project();
    expect(p).toBeInstanceOf(JsProject);
  });
});
