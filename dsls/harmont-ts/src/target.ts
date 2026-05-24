const cache = new Map<symbol, unknown>();

export function target<T>(_name: string, fn: () => T): () => T {
  const key = Symbol(_name);
  return () => {
    if (!cache.has(key)) {
      cache.set(key, fn());
    }
    return cache.get(key) as T;
  };
}

export function clearTargetCache(): void {
  cache.clear();
}
