import { createHash } from "node:crypto";
import {
  readFileSync,
  readdirSync,
  statSync,
  existsSync,
} from "node:fs";
import { join, resolve as resolvePath, relative } from "node:path";
import type { PipelineIR } from "./pipeline.js";

const NUL = "\0";

export interface CacheKeyOptions {
  readonly pipelineOrg: string;
  readonly pipelineSlug: string;
  readonly now: number;
  readonly basePath: string;
  readonly env: Readonly<Record<string, string>>;
}

export function resolvePipelineCacheKeys(
  graph: PipelineIR["graph"],
  opts: CacheKeyOptions,
): void {
  const nodes = graph.nodes;
  const edges = graph.edges;

  const keyByIdx = new Map<number, string>();
  for (let i = 0; i < nodes.length; i++) {
    keyByIdx.set(i, nodes[i].step.key as string);
  }

  const parentKeyMap = new Map<string, string>();
  for (const [src, dst, kind] of edges) {
    if (kind === "builds_in") {
      parentKeyMap.set(keyByIdx.get(dst)!, keyByIdx.get(src)!);
    }
  }

  const resolved = new Map<string, string>();

  for (const node of nodes) {
    const step = node.step;
    const cache = step.cache as Record<string, unknown> | undefined;
    if (!cache || cache.policy === "none") continue;

    const cmd = (step.cmd as string) ?? "";
    const stepKey = step.key as string;
    const parentStepKey = parentKeyMap.get(stepKey);
    const parentResolved = lookupParent(parentStepKey, resolved);
    const policyRes = resolvePolicy(cache, cmd, opts);

    const key = sha256hex(
      opts.pipelineOrg +
        NUL +
        opts.pipelineSlug +
        NUL +
        stepKey +
        NUL +
        parentResolved +
        NUL +
        policyRes,
    );

    cache.key = key;
    resolved.set(stepKey, key);
  }
}

function lookupParent(
  parentStepKey: string | undefined,
  resolved: Map<string, string>,
): string {
  if (parentStepKey == null) return "scratch";
  const key = resolved.get(parentStepKey);
  if (key == null) {
    throw new Error(
      `step references builds_in "${parentStepKey}" which has no cached key (parent must be defined upstream and cached)`,
    );
  }
  return key;
}

function resolvePolicy(
  cache: Record<string, unknown>,
  cmd: string,
  opts: CacheKeyOptions,
): string {
  const policy = cache.policy as string;

  if (policy === "none") return "none";

  if (policy === "forever") {
    const envKeys = (cache.env_keys as string[]) ?? [];
    return "forever-" + sha256hex(cmd + NUL + envSubset(envKeys, opts.env));
  }

  if (policy === "ttl") {
    const duration = cache.duration_seconds as number;
    const bucket = Math.floor(opts.now / duration);
    const envKeys = (cache.env_keys as string[]) ?? [];
    return (
      "ttl-" +
      bucket +
      "-" +
      sha256hex(cmd + NUL + envSubset(envKeys, opts.env))
    );
  }

  if (policy === "on_change") {
    const paths = (cache.paths as string[]) ?? [];
    const resolvedPaths: string[] = [];
    for (const p of [...paths].sort()) {
      if (/[*?[]/.test(p)) {
        resolvedPaths.push(...globPaths(opts.basePath, p));
      } else {
        const full = resolvePath(opts.basePath, p);
        if (existsSync(full)) resolvedPaths.push(full);
      }
    }
    const pre = resolvedPaths.map((r) => pathHash(r) + NUL).join("");
    return "sha-" + sha256hex(pre);
  }

  if (policy === "compose") {
    const subs = cache.sub_policies as Record<string, unknown>[];
    const parts = subs.map((sub) =>
      sub.policy !== "none" ? resolvePolicy(sub, cmd, opts) : "none",
    );
    return "compose-" + sha256hex(parts.join(""));
  }

  throw new Error(`resolve-policy-key: unknown policy "${policy}"`);
}

function envSubset(
  envKeys: readonly string[],
  env: Readonly<Record<string, string>>,
): string {
  const sorted = [...envKeys].sort();
  return sorted.map((k) => k + "=" + (env[k] ?? "") + NUL).join("");
}

function pathHash(fullPath: string): string {
  const stat = statSync(fullPath);
  if (stat.isFile()) {
    return sha256hex(readFileSync(fullPath));
  }
  if (stat.isDirectory()) {
    const h = createHash("sha256");
    const files = walkDir(fullPath).sort();
    for (const child of files) {
      const rel = relative(fullPath, child).split("\\").join("/");
      h.update(rel, "utf8");
      h.update(NUL);
      h.update(readFileSync(child));
      h.update(NUL);
    }
    return h.digest("hex");
  }
  throw new Error(`on_change path does not exist: ${fullPath}`);
}

function walkDir(dir: string): string[] {
  const results: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...walkDir(full));
    } else if (entry.isFile()) {
      results.push(full);
    }
  }
  return results;
}

function globPaths(basePath: string, pattern: string): string[] {
  const parts = pattern.split("/");
  let candidates = [basePath];

  for (const part of parts) {
    const next: string[] = [];
    for (const dir of candidates) {
      if (!existsSync(dir) || !statSync(dir).isDirectory()) continue;
      if (part === "**") {
        next.push(dir);
        next.push(...walkDir(dir).map((f) => join(f, "..")));
      } else if (/[*?[]/.test(part)) {
        const re = globPartToRegex(part);
        for (const entry of readdirSync(dir, { withFileTypes: true })) {
          if (re.test(entry.name)) {
            next.push(join(dir, entry.name));
          }
        }
      } else {
        const full = join(dir, part);
        if (existsSync(full)) next.push(full);
      }
    }
    candidates = next;
  }

  return candidates
    .filter((p) => existsSync(p) && statSync(p).isFile())
    .sort();
}

function globPartToRegex(part: string): RegExp {
  let re = "^";
  for (const ch of part) {
    if (ch === "*") re += ".*";
    else if (ch === "?") re += ".";
    else re += ch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }
  re += "$";
  return new RegExp(re);
}

function sha256hex(data: string | Buffer): string {
  return createHash("sha256").update(data).digest("hex");
}
