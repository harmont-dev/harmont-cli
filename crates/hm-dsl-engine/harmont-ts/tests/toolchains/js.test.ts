import { describe, expect, it } from "vitest";
import { js, ts, JsProject } from "../../src/toolchains/js.js";
import { sh } from "../../src/step.js";
import { pipeline } from "../../src/pipeline.js";

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
      'pm="npm" requires runtime="node"',
    );
  });

  it("rejects pm=pnpm with runtime=bun", () => {
    expect(() => js.project({ pm: "pnpm", runtime: "bun" })).toThrow(
      'pm="pnpm" requires runtime="node"',
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
  it("chain is: scratch → apt-base → node-install → pnpm-global → pnpm-deps", () => {
    const p = js.project({ pm: "pnpm" });
    const pnpmDeps = p.install();
    expect(pnpmDeps._cmd).toContain("pnpm install --frozen-lockfile");

    const pnpmGlobal = pnpmDeps._parent!;
    expect(pnpmGlobal._cmd).toContain("npm install -g pnpm");

    const nodeInstall = pnpmGlobal._parent!;
    expect(nodeInstall._cmd).toContain("nodejs");

    const aptBase = nodeInstall._parent!;
    expect(aptBase._cmd).toContain("apt-get");

    const root = aptBase._parent!;
    expect(root._cmd).toBeNull();
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
// Actions
// ---------------------------------------------------------------------------

describe("js.project actions", () => {
  it("run() executes arbitrary script via npm run", () => {
    const p = js.project();
    const r = p.run("typecheck");
    expect(r._cmd).toContain("npm run typecheck");
  });

  it("run() uses pnpm run for pnpm PM", () => {
    const p = js.project({ pm: "pnpm" });
    const r = p.run("typecheck");
    expect(r._cmd).toContain("pnpm run typecheck");
  });

  it("run() uses bun run for bun runtime", () => {
    const p = js.project({ runtime: "bun" });
    const r = p.run("typecheck");
    expect(r._cmd).toContain("bun run typecheck");
  });

  it("run() uses deno task for deno runtime", () => {
    const p = js.project({ runtime: "deno" });
    const r = p.run("typecheck");
    expect(r._cmd).toContain("deno task typecheck");
  });

  it("test is sugar for run('test')", () => {
    const p = js.project();
    const t = p.test();
    expect(t._cmd).toContain("npm run test");
    expect(t._parent).toBe(p.install());
  });

  it("build is sugar for run('build')", () => {
    const p = js.project();
    expect(p.build()._cmd).toContain("npm run build");
  });

  it("lint is sugar for run('lint')", () => {
    const p = js.project();
    expect(p.lint()._cmd).toContain("npm run lint");
  });

  it("fmt is sugar for run('fmt')", () => {
    const p = js.project();
    expect(p.fmt()._cmd).toContain("npm run fmt");
  });

  it("typecheck is sugar for run('typecheck')", () => {
    const p = js.project();
    expect(p.typecheck()._cmd).toContain("npm run typecheck");
  });

  it("actions respect custom path", () => {
    const p = js.project({ path: "packages/ui" });
    expect(p.run("test")._cmd).toContain("cd packages/ui");
  });

  it("actions accept step options (label, timeoutSeconds)", () => {
    const p = js.project();
    const t = p.test({ label: "my test", timeoutSeconds: 300 });
    expect(t._label).toBe("my test");
    expect(t._timeoutSeconds).toBe(300);
  });

  it("default labels use :node: tag for node runtime", () => {
    const p = js.project();
    expect(p.test()._label).toBe(":node: test");
    expect(p.lint()._label).toBe(":node: lint");
  });

  it("default labels use :bun: tag for bun runtime", () => {
    const p = js.project({ runtime: "bun" });
    expect(p.test()._label).toBe(":bun: test");
    expect(p.lint()._label).toBe(":bun: lint");
  });

  it("default labels use :deno: tag for deno runtime", () => {
    const p = js.project({ runtime: "deno" });
    expect(p.test()._label).toBe(":deno: test");
    expect(p.lint()._label).toBe(":deno: lint");
  });

  it("run label uses script name", () => {
    const p = js.project();
    expect(p.run("typecheck")._label).toBe(":node: typecheck");
    expect(p.run("coverage")._label).toBe(":node: coverage");
  });
});

// ---------------------------------------------------------------------------
// Pipeline IR
// ---------------------------------------------------------------------------

describe("js.project pipeline IR", () => {
  it("produces valid IR for node+npm", () => {
    const p = js.project();
    const ir = pipeline(p.test(), p.lint(), { defaultImage: "ubuntu:24.04" });
    expect(ir.version).toBe("0");
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
  });

  it("produces valid IR for node+pnpm", () => {
    const p = js.project({ pm: "pnpm" });
    const ir = pipeline(p.test(), p.lint(), { defaultImage: "ubuntu:24.04" });
    expect(ir.version).toBe("0");
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(5);
  });

  it("produces valid IR for bun+bun", () => {
    const p = js.project({ runtime: "bun" });
    const ir = pipeline(p.test(), p.lint(), { defaultImage: "ubuntu:24.04" });
    expect(ir.version).toBe("0");
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
  });

  it("produces valid IR for deno", () => {
    const p = js.project({ runtime: "deno" });
    const ir = pipeline(p.test(), p.lint(), { defaultImage: "ubuntu:24.04" });
    expect(ir.version).toBe("0");
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(4);
  });

  it("test and lint share install as parent (fan-out)", () => {
    const p = js.project();
    const t = p.test();
    const l = p.lint();
    expect(t._parent).toBe(p.install());
    expect(l._parent).toBe(p.install());
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
