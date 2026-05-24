import type { Step } from "./step.js";

const cache = new Map<symbol, Step>();

export function target(
  _name: string,
  fn: () => Step,
): () => Step {
  const key = Symbol(_name);
  return () => {
    if (!cache.has(key)) {
      cache.set(key, fn());
    }
    return cache.get(key)!;
  };
}

export function clearTargetCache(): void {
  cache.clear();
}
