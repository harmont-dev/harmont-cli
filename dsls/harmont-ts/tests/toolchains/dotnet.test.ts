import { describe, expect, it } from "vitest";
import { dotnet } from "../../src/toolchains/dotnet.js";
import { pipeline } from "../../src/pipeline.js";

describe("dotnet factory", () => {
  it("returns a DotnetProject with defaults", () => {
    const d = dotnet();
    expect(d.path).toBe(".");
    expect(d.install()._cmd).toContain("dotnet --info");
  });

  it("accepts channel", () => {
    const d = dotnet({ channel: "LTS" });
    expect(d.install()._cmd).toContain("--channel LTS");
  });

  it("rejects invalid channel", () => {
    expect(() => dotnet({ channel: "bad" })).toThrow("invalid channel");
  });
});

describe("dotnet actions", () => {
  it("build runs dotnet build", () => {
    expect(dotnet().build()._cmd).toContain("dotnet build");
  });

  it("test runs dotnet test", () => {
    expect(dotnet().test()._cmd).toContain("dotnet test");
  });

  it("fmt runs dotnet format --verify-no-changes", () => {
    expect(dotnet().fmt()._cmd).toContain("dotnet format --verify-no-changes");
  });

  it("default labels use :dotnet: prefix", () => {
    const d = dotnet();
    expect(d.build()._label).toBe(":dotnet: build");
    expect(d.test()._label).toBe(":dotnet: test");
    expect(d.fmt()._label).toBe(":dotnet: fmt");
  });
});

describe("dotnet in pipeline", () => {
  it("produces valid IR", () => {
    const d = dotnet();
    const ir = pipeline(d.build(), d.test(), { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(3);
  });
});
