import { describe, expect, it } from "vitest";
import { go } from "../../src/toolchains/go.js";
import { sh } from "../../src/step.js";
import { pipeline } from "../../src/pipeline.js";

describe("go factory", () => {
  it("returns a GoToolchain with defaults", () => {
    const g = go();
    expect(g.path).toBe(".");
    expect(g.install()._cmd).toContain("go version");
  });

  it("accepts path and version", () => {
    const g = go({ path: "cmd/server", version: "1.22" });
    expect(g.path).toBe("cmd/server");
    expect(g.install()._cmd).toContain("go1.22");
  });

  it("rejects invalid version", () => {
    expect(() => go({ version: "abc" })).toThrow("invalid version");
  });

  it("accepts two-part version", () => {
    expect(() => go({ version: "1.23" })).not.toThrow();
  });
});

describe("go actions", () => {
  it("build runs go build", () => {
    const g = go();
    expect(g.build()._cmd).toContain("go build ./...");
  });

  it("test runs go test", () => {
    const g = go();
    expect(g.test()._cmd).toContain("go test ./...");
  });

  it("vet runs go vet", () => {
    const g = go();
    expect(g.vet()._cmd).toContain("go vet ./...");
  });

  it("fmt runs gofmt check", () => {
    const g = go();
    expect(g.fmt()._cmd).toContain("gofmt -l");
  });

  it("actions chain from install step", () => {
    const g = go();
    expect(g.build()._parent).toBe(g.install());
  });

  it("accepts step options", () => {
    const g = go();
    const t = g.test({ label: "my test", timeoutSeconds: 300 });
    expect(t._label).toBe("my test");
    expect(t._timeoutSeconds).toBe(300);
  });

  it("default labels use :go: prefix", () => {
    const g = go();
    expect(g.build()._label).toBe(":go: build");
    expect(g.test()._label).toBe(":go: test");
    expect(g.vet()._label).toBe(":go: vet");
    expect(g.fmt()._label).toBe(":go: fmt");
  });
});

describe("go install chain", () => {
  it("chain is: scratch → apt-base → go-install", () => {
    const g = go();
    const install = g.install();
    expect(install._cmd).toContain("go version");

    const aptBase = install._parent!;
    expect(aptBase._cmd).toContain("apt-get");

    const root = aptBase._parent!;
    expect(root._cmd).toBeNull();
  });

  it("accepts base step", () => {
    const base = sh("custom base");
    const g = go({ base });
    expect(g.install()._parent).toBe(base);
  });

  it("accepts custom image", () => {
    const g = go({ image: "debian:12" });
    const install = g.install();
    const aptBase = install._parent!;
    const root = aptBase._parent!;
    expect(root._image).toBe("debian:12");
  });
});

describe("go in pipeline", () => {
  it("produces valid IR", () => {
    const g = go();
    const ir = pipeline([g.build(), g.test()], { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(3);
    expect(ir.version).toBe("0");
  });
});
