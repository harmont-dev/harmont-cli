import { scratch, type Step, type StepOptions } from "../step.js";
import { ttl, type CachePolicy } from "../cache.js";

const APT_TTL_SECONDS = 86400; // 1 day

export function aptInstallCmd(packages: readonly string[]): string {
  return `apt-get update && apt-get install -y ${packages.join(" ")}`;
}

export function nodeInstallCmd(version: string): string {
  const major = version.replace(/\.x$/, "");
  return `curl -fsSL https://deb.nodesource.com/setup_${major}.x | bash - && apt-get install -y nodejs`;
}

export function aptBase(opts: {
  packages: readonly string[];
  image?: string;
  label?: string;
}): Step {
  return scratch({ image: opts.image }).sh(aptInstallCmd(opts.packages), {
    label: opts.label ?? ":apt: base",
    cache: ttl(APT_TTL_SECONDS),
  });
}

export function makeInstallChain(opts: {
  aptPackages: readonly string[];
  installCmd: string;
  installCache: CachePolicy;
  langTag: string;
  installTag: string;
  image?: string;
  base?: Step;
}): Step {
  let parent: Step;
  if (opts.base == null) {
    parent = scratch({ image: opts.image }).sh(aptInstallCmd(opts.aptPackages), {
      label: `:${opts.langTag}: apt-base`,
      cache: ttl(APT_TTL_SECONDS),
    });
  } else {
    parent = opts.base;
  }
  return parent.sh(opts.installCmd, {
    label: `:${opts.langTag}: ${opts.installTag}`,
    cache: opts.installCache,
  });
}
