import { describe, expect, it } from "vitest";
import { slugifyLabel, hashKey, resolveKeys } from "../src/keys.js";
import { scratch, sh } from "../src/step.js";

describe("slugifyLabel", () => {
  it("lowercases and replaces non-alnum with dashes", () => {
    expect(slugifyLabel("Hello World")).toBe("hello-world");
  });

  it("strips emoji shortcodes", () => {
    expect(slugifyLabel(":rust: build")).toBe("build");
  });

  it("trims leading/trailing dashes", () => {
    expect(slugifyLabel("--hello--")).toBe("hello");
  });

  it("returns empty string for non-ASCII-only labels", () => {
    expect(slugifyLabel("构建")).toBe("");
  });

  it("handles mixed emoji and text", () => {
    expect(slugifyLabel(":node: deps install")).toBe("deps-install");
  });
});

describe("hashKey", () => {
  it("returns a 12-char hex string", () => {
    const key = hashKey("parent", "echo hello", 0);
    expect(key).toMatch(/^[0-9a-f]{12}$/);
  });

  it("is deterministic", () => {
    expect(hashKey("p", "cmd", 1)).toBe(hashKey("p", "cmd", 1));
  });

  it("differs for different inputs", () => {
    expect(hashKey("p", "cmd1", 0)).not.toBe(hashKey("p", "cmd2", 0));
  });
});

describe("resolveKeys", () => {
  it("uses slugified label when unique", () => {
    const a = scratch().sh("install", { label: "install" });
    const b = a.sh("build", { label: "build" });
    const keys = resolveKeys([a, b]);
    expect(keys.get(a._id)).toBe("install");
    expect(keys.get(b._id)).toBe("build");
  });

  it("falls back to hash when label slugs collide", () => {
    const a = scratch().sh("cmd a", { label: "test" });
    const b = scratch().sh("cmd b", { label: "test" });
    const keys = resolveKeys([a, b]);
    expect(keys.get(a._id)).toMatch(/^[0-9a-f]{12}$/);
    expect(keys.get(b._id)).toMatch(/^[0-9a-f]{12}$/);
    expect(keys.get(a._id)).not.toBe(keys.get(b._id));
  });

  it("explicit key override wins over label", () => {
    const a = scratch().sh("echo", { label: "hello", key: "my-key" });
    const keys = resolveKeys([a]);
    expect(keys.get(a._id)).toBe("my-key");
  });

  it("explicit override reserves slug, colliding label falls back to hash", () => {
    const a = scratch().sh("cmd a", { label: "build", key: "build" });
    const b = scratch().sh("cmd b", { label: "build" });
    const keys = resolveKeys([a, b]);
    expect(keys.get(a._id)).toBe("build");
    expect(keys.get(b._id)).toMatch(/^[0-9a-f]{12}$/);
  });

  it("falls back to hash when label is empty after slugify", () => {
    const a = scratch().sh("echo", { label: "构建" });
    const keys = resolveKeys([a]);
    expect(keys.get(a._id)).toMatch(/^[0-9a-f]{12}$/);
  });

  it("uses parent key for hash computation", () => {
    const parent = scratch().sh("install", { label: "install" });
    const child = parent.sh("build");
    const keys = resolveKeys([parent, child]);
    expect(keys.get(parent._id)).toBe("install");
    const expected = hashKey("install", "build", 1);
    expect(keys.get(child._id)).toBe(expected);
  });
});
