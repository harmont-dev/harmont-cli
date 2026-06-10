import { describe, expect, it, beforeEach } from "vitest";
import {
  target,
  clearTargetCache,
  renderDynamicTarget,
} from "../src/target.js";
import { sh } from "../src/step.js";
import { forever } from "../src/cache.js";

beforeEach(() => {
  clearTargetCache();
});

describe("target", () => {
  it("returns a factory function", () => {
    const nodeBase = target("node-base", () => {
      return sh("apt-get install -y nodejs", { cache: forever() });
    });
    expect(typeof nodeBase).toBe("function");
  });

  it("factory returns the step", () => {
    const nodeBase = target("node-base", () => {
      return sh("apt-get install -y nodejs");
    });
    const step = nodeBase();
    expect(step._cmd).toBe("apt-get install -y nodejs");
  });

  it("memoizes return value", () => {
    let callCount = 0;
    const nodeBase = target("node-base", () => {
      callCount++;
      return sh("install");
    });
    const a = nodeBase();
    const b = nodeBase();
    expect(a).toBe(b);
    expect(callCount).toBe(1);
  });

  it("clearTargetCache resets memoization", () => {
    let callCount = 0;
    const nodeBase = target("node-base", () => {
      callCount++;
      return sh("install");
    });
    nodeBase();
    clearTargetCache();
    nodeBase();
    expect(callCount).toBe(2);
  });

  it("different targets are independent", () => {
    const a = target("a", () => sh("cmd-a"));
    const b = target("b", () => sh("cmd-b"));
    expect(a()._cmd).toBe("cmd-a");
    expect(b()._cmd).toBe("cmd-b");
  });

  it("target can build on another target", () => {
    const base = target("base", () => sh("install base"));
    const app = target("app", () => base().sh("install app"));
    const step = app();
    expect(step._cmd).toBe("install app");
    expect(step._parent!._cmd).toBe("install base");
  });

  it("memoizes non-Step values (generic)", () => {
    const factory = target("my-obj", () => ({ value: Math.random() }));
    const a = factory();
    const b = factory();
    expect(a).toBe(b);
    expect(a.value).toBe(b.value);
  });

  it("defers dynamic target evaluation", () => {
    let callCount = 0;
    const chooseBuild = target(
      "choose-build",
      () => {
        callCount++;
        return sh("npm run build");
      },
      { dynamic: true },
    );

    const placeholder = chooseBuild();
    expect(callCount).toBe(0);
    expect(placeholder._dynamicTargetName).toBe("choose-build");

    const rendered = renderDynamicTarget("choose-build");
    expect(callCount).toBe(1);
    expect(rendered.graph.nodes[0].step.eval).toEqual({
      type: "cmd",
      cmd: "npm run build",
    });
  });

  it("renders dynamic target groups", () => {
    target(
      "checks",
      () => [sh("npm test"), sh("npm run lint")],
      { dynamic: true },
    );

    const rendered = renderDynamicTarget("checks");
    expect(rendered.graph.nodes.map((node) => node.step.eval)).toEqual([
      { type: "cmd", cmd: "npm test" },
      { type: "cmd", cmd: "npm run lint" },
    ]);
  });

  it("rejects unknown dynamic target names", () => {
    expect(() => renderDynamicTarget("missing")).toThrow(
      "dynamic target 'missing' not found",
    );
  });
});
