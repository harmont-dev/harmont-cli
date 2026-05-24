import { describe, expect, it } from "vitest";
import { push, pullRequest, schedule } from "../src/triggers.js";

describe("push", () => {
  it("creates a branch trigger from string", () => {
    const t = push({ branch: "main" });
    expect(t.toJSON()).toEqual({ event: "push", branches: ["main"] });
  });

  it("creates a branch trigger from array", () => {
    const t = push({ branch: ["main", "develop"] });
    expect(t.toJSON()).toEqual({ event: "push", branches: ["main", "develop"] });
  });

  it("creates a tag trigger", () => {
    const t = push({ tag: "v*" });
    expect(t.toJSON()).toEqual({ event: "push", tags: ["v*"] });
  });

  it("rejects when neither branch nor tag", () => {
    expect(() => push({} as any)).toThrow("exactly one of branch or tag");
  });

  it("rejects when both branch and tag", () => {
    expect(() => push({ branch: "main", tag: "v*" } as any)).toThrow(
      "exactly one of branch or tag",
    );
  });
});

describe("pullRequest", () => {
  it("uses default types when none specified", () => {
    const t = pullRequest();
    expect(t.toJSON()).toEqual({
      event: "pull_request",
      types: ["opened", "synchronize", "reopened"],
    });
  });

  it("accepts branch filter", () => {
    const t = pullRequest({ branches: ["main"] });
    const json = t.toJSON();
    expect(json.branches).toEqual(["main"]);
  });

  it("accepts custom types", () => {
    const t = pullRequest({ types: ["opened", "closed"] });
    expect(t.toJSON().types).toEqual(["opened", "closed"]);
  });

  it("rejects invalid types", () => {
    expect(() => pullRequest({ types: ["invalid" as any] })).toThrow("unknown pull_request type");
  });

  it("rejects empty types", () => {
    expect(() => pullRequest({ types: [] })).toThrow("types must be non-empty");
  });
});

describe("schedule", () => {
  it("creates a cron trigger", () => {
    const t = schedule("0 4 * * *");
    expect(t.toJSON()).toEqual({ event: "schedule", cron: "0 4 * * *" });
  });

  it("rejects invalid cron", () => {
    expect(() => schedule("not a cron")).toThrow("invalid cron expression");
  });
});
