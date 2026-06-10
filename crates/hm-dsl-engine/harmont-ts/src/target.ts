import { pipeline, type PipelineIR } from "./pipeline.js";
import { dynamicTarget, type Step } from "./step.js";

const cache = new Map<symbol, unknown>();
const dynamicTargets = new Map<string, () => Step | readonly Step[]>();

export interface TargetOptions {
  readonly dynamic?: boolean;
}

export function target<T>(
  name: string,
  fn: () => T,
  opts?: { readonly dynamic?: false },
): () => T;
export function target(
  name: string,
  fn: () => Step | readonly Step[],
  opts: { readonly dynamic: true },
): () => Step;
export function target<T>(
  name: string,
  fn: () => T,
  opts?: TargetOptions,
): () => T | Step {
  const key = Symbol(name);
  if (opts?.dynamic) {
    dynamicTargets.set(name, fn as () => Step | readonly Step[]);
    return () => dynamicTarget(name);
  }

  return () => {
    if (!cache.has(key)) {
      cache.set(key, fn());
    }
    return cache.get(key) as T;
  };
}

export function clearTargetCache(): void {
  cache.clear();
  dynamicTargets.clear();
}

export function renderDynamicTarget(
  name: string,
  env: Readonly<Record<string, string>> = {},
): PipelineIR {
  const fn = dynamicTargets.get(name);
  if (fn == null) {
    throw new Error(`hm: dynamic target '${name}' not found`);
  }

  cache.clear();
  const result = fn();
  const leaves = Array.isArray(result) ? [...result] : [result as Step];
  return pipeline(leaves, { env });
}
