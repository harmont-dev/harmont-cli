import { createHash } from "node:crypto";
import type { Step } from "./step.js";

const EMOJI_SHORTCODE_RE = /:[a-z0-9_+-]+:/g;
const NON_ALNUM_RE = /[^a-z0-9]+/g;

export function slugifyLabel(label: string): string {
  let s = label.toLowerCase();
  s = s.replace(EMOJI_SHORTCODE_RE, " ");
  s = s.replace(NON_ALNUM_RE, "-");
  s = s.replace(/^-+|-+$/g, "");
  return s;
}

export function hashKey(parentKey: string, cmd: string, position: number): string {
  const h = createHash("sha256");
  h.update(parentKey, "utf8");
  h.update("\0");
  h.update(cmd, "utf8");
  h.update("\0");
  h.update(String(position), "utf8");
  return h.digest("hex").slice(0, 12);
}

export function resolveKeys(steps: readonly Step[]): Map<number, string> {
  const overrides = new Map<number, string>();
  const naturalSlugs = new Map<number, string>();

  for (const s of steps) {
    if (s._keyOverride != null) {
      overrides.set(s._id, s._keyOverride);
    }
    if (s._label != null) {
      const slug = slugifyLabel(s._label);
      if (slug) {
        naturalSlugs.set(s._id, slug);
      }
    }
  }

  const reserved = new Set(overrides.values());

  const slugCounts = new Map<string, number>();
  for (const slug of naturalSlugs.values()) {
    slugCounts.set(slug, (slugCounts.get(slug) ?? 0) + 1);
  }

  const labelSlugs = new Map<number, string>();
  for (const [id, slug] of naturalSlugs) {
    if (!overrides.has(id)) {
      labelSlugs.set(id, slug);
    }
  }

  const keys = new Map<number, string>();
  for (let position = 0; position < steps.length; position++) {
    const s = steps[position];
    const sid = s._id;

    if (overrides.has(sid)) {
      keys.set(sid, overrides.get(sid)!);
      continue;
    }

    const candidateSlug = labelSlugs.get(sid);
    if (
      candidateSlug != null &&
      !reserved.has(candidateSlug) &&
      slugCounts.get(candidateSlug) === 1
    ) {
      keys.set(sid, candidateSlug);
      reserved.add(candidateSlug);
      continue;
    }

    let parentKey = "";
    if (s._parent != null && keys.has(s._parent._id)) {
      parentKey = keys.get(s._parent._id)!;
    }
    keys.set(sid, hashKey(parentKey, s._cmd ?? "", position));
  }

  return keys;
}
