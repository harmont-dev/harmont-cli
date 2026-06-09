import { describe, expect, it } from "vitest";
import { scratch, sh, wait, Step, timeout } from "../src/step.js";

describe("scratch", () => {
  it("creates a root step with no cmd or parent", () => {
    const s = scratch();
    expect(s).toBeInstanceOf(Step);
    expect(s._cmd).toBeNull();
    expect(s._parent).toBeNull();
    expect(s._isWait).toBe(false);
  });
});

describe("sh", () => {
  it("creates a step with cmd and implicit scratch parent", () => {
    const s = sh("echo hello");
    expect(s._cmd).toBe("echo hello");
    expect(s._parent).not.toBeNull();
    expect(s._parent!._cmd).toBeNull();
  });

  it("passes options through", () => {
    const s = timeout(600, sh("make", {
      label: "build",
      env: { CI: "true" },
      image: "ubuntu:24.04",
      key: "my-key",
    }));
    expect(s._label).toBe("build");
    expect(s._timeoutSeconds).toBe(600);
    expect(s._env).toEqual({ CI: "true" });
    expect(s._image).toBe("ubuntu:24.04");
    expect(s._keyOverride).toBe("my-key");
  });

  it("prepends cd when cwd is set", () => {
    const s = sh("npm test", { cwd: "packages/app" });
    expect(s._cmd).toBe("cd packages/app && npm test");
  });

  it("rejects empty cwd", () => {
    expect(() => sh("echo", { cwd: "" })).toThrow("cwd must be a non-empty path");
  });
});

describe("Step.sh", () => {
  it("chains a child step with parent pointer", () => {
    const parent = sh("install");
    const child = parent.sh("build");
    expect(child._cmd).toBe("build");
    expect(child._parent).toBe(parent);
  });

  it("inherits image from scratch parent", () => {
    const base = scratch({ image: "alpine:3.20" });
    const child = base.sh("echo");
    expect(child._image).toBe("alpine:3.20");
  });

  it("does not inherit image from command parent", () => {
    const parent = sh("install", { image: "ubuntu:24.04" });
    const child = parent.sh("build");
    expect(child._image).toBeUndefined();
  });

  it("explicit image overrides inherited image", () => {
    const base = scratch({ image: "alpine:3.20" });
    const child = base.sh("echo", { image: "ubuntu:24.04" });
    expect(child._image).toBe("ubuntu:24.04");
  });
});

describe("Step.fork", () => {
  it("creates a cmd-less step with parent pointer", () => {
    const parent = sh("install");
    const branch = parent.fork({ label: "branch-a" });
    expect(branch._cmd).toBeNull();
    expect(branch._parent).toBe(parent);
    expect(branch._label).toBe("branch-a");
  });
});

describe("wait", () => {
  it("creates a wait step", () => {
    const w = wait();
    expect(w._isWait).toBe(true);
    expect(w._continueOnFailure).toBe(false);
  });

  it("accepts continueOnFailure", () => {
    const w = wait({ continueOnFailure: true });
    expect(w._continueOnFailure).toBe(true);
  });
});

describe("step identity", () => {
  it("each step gets a unique id", () => {
    const a = sh("a");
    const b = sh("b");
    expect(a._id).not.toBe(b._id);
  });
});
