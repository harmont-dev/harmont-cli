// Shared cargo-argument assembly for the Rust toolchain helper.
//
// Mirrors harmont-py/harmont/_cargo.py exactly so both DSLs emit identical
// cargo command strings. User-supplied *values* are shell-quoted; raw `flags`
// pass through verbatim. `exclude` pairs with `--workspace` (cargo requires it).

export interface CargoOpts {
  readonly workspace?: boolean;
  readonly packages?: readonly string[];
  readonly exclude?: readonly string[];
  readonly allFeatures?: boolean;
  readonly noDefaultFeatures?: boolean;
  readonly features?: readonly string[];
  readonly target?: string;
  readonly allTargets?: boolean;
  readonly release?: boolean;
  readonly profile?: string;
  readonly locked?: boolean;
  readonly flags?: readonly string[];
}

// POSIX single-quote escaping, byte-for-byte identical to Python's shlex.quote:
// safe characters are left bare; everything else is wrapped in single quotes
// with embedded single quotes rendered as '"'"'. The empty string becomes ''.
const SAFE = /^[A-Za-z0-9_@%+=:,./-]+$/;
export function shQuote(s: string): string {
  if (s.length === 0) return "''";
  if (SAFE.test(s)) return s;
  return "'" + s.replace(/'/g, "'\"'\"'") + "'";
}

function validate(o: CargoOpts): void {
  if (o.allFeatures && ((o.features?.length ?? 0) > 0 || o.noDefaultFeatures)) {
    throw new Error(
      "rust: --all-features conflicts with features/noDefaultFeatures\n" +
        `  observed: allFeatures=true, features=${JSON.stringify(o.features ?? [])}, ` +
        `noDefaultFeatures=${o.noDefaultFeatures ?? false}\n` +
        "  → pass allFeatures alone, or list explicit features without allFeatures",
    );
  }
  if (o.release && o.profile !== undefined) {
    throw new Error(
      "rust: release conflicts with profile\n" +
        `  observed: release=true, profile=${JSON.stringify(o.profile)}\n` +
        '  → use profile: "release" (identical effect) or drop one',
    );
  }
  if (o.exclude?.length) {
    if (o.packages?.length) {
      throw new Error(
        "rust: exclude cannot combine with packages\n" +
          `  observed: packages=${JSON.stringify(o.packages)}, exclude=${JSON.stringify(o.exclude)}\n` +
          "  → --exclude pairs with --workspace; packages already selects explicitly, so drop one",
      );
    }
    if (!o.workspace) {
      throw new Error(
        "rust: exclude requires workspace\n" +
          `  observed: exclude=${JSON.stringify(o.exclude)} without workspace=true\n` +
          "  → cargo --exclude only applies to --workspace; pass workspace: true",
      );
    }
  }
}

export function cargoFlags(o: CargoOpts): string {
  validate(o);
  const toks: string[] = [];

  if (o.packages?.length) {
    for (const p of o.packages) toks.push(`-p ${shQuote(p)}`);
  } else if (o.workspace) {
    toks.push("--workspace");
    for (const e of o.exclude ?? []) toks.push(`--exclude ${shQuote(e)}`);
  }

  if (o.allTargets) toks.push("--all-targets");

  if (o.allFeatures) {
    toks.push("--all-features");
  } else {
    if (o.noDefaultFeatures) toks.push("--no-default-features");
    if (o.features?.length) toks.push(`--features ${shQuote(o.features.join(","))}`);
  }

  if (o.target !== undefined) toks.push(`--target ${shQuote(o.target)}`);

  if (o.profile !== undefined) toks.push(`--profile ${shQuote(o.profile)}`);
  else if (o.release) toks.push("--release");

  if (o.locked ?? true) toks.push("--locked");

  if (o.flags?.length) toks.push(...o.flags);

  return toks.length ? " " + toks.join(" ") : "";
}
