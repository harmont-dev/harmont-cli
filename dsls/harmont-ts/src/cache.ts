export interface CacheForever {
  readonly kind: "forever";
  readonly envKeys: readonly string[];
}

export interface CacheTTL {
  readonly kind: "ttl";
  readonly durationSeconds: number;
  readonly envKeys: readonly string[];
}

export interface CacheOnChange {
  readonly kind: "on_change";
  readonly paths: readonly string[];
}

export interface CacheCompose {
  readonly kind: "compose";
  readonly policies: readonly CachePolicy[];
}

export type CachePolicy = CacheForever | CacheTTL | CacheOnChange | CacheCompose;

export function forever(opts?: { envKeys?: string[] }): CacheForever {
  return { kind: "forever", envKeys: opts?.envKeys ?? [] };
}

export function ttl(
  durationSeconds: number,
  opts?: { envKeys?: string[] },
): CacheTTL {
  return { kind: "ttl", durationSeconds, envKeys: opts?.envKeys ?? [] };
}

export function onChange(...paths: string[]): CacheOnChange {
  return { kind: "on_change", paths };
}

export function compose(...policies: CachePolicy[]): CacheCompose {
  return { kind: "compose", policies };
}
