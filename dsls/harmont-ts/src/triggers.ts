export type Trigger = PushTrigger | PullRequestTrigger | ScheduleTrigger;

function normalizeGlobs(
  value: string | readonly string[] | undefined,
): string[] | undefined {
  if (value === undefined) return undefined;
  if (typeof value === "string") return [value];
  return [...value];
}

export class PushTrigger {
  readonly branches: string[] | undefined;
  readonly tags: string[] | undefined;

  constructor(branches: string[] | undefined, tags: string[] | undefined) {
    this.branches = branches;
    this.tags = tags;
  }

  toJSON(): Record<string, unknown> {
    const out: Record<string, unknown> = { event: "push" };
    if (this.branches !== undefined) out.branches = this.branches;
    if (this.tags !== undefined) out.tags = this.tags;
    return out;
  }
}

export function push(
  opts: { branch: string | string[]; tag?: undefined } | { tag: string | string[]; branch?: undefined },
): PushTrigger {
  const branch = "branch" in opts ? opts.branch : undefined;
  const tag = "tag" in opts ? opts.tag : undefined;
  const branches = normalizeGlobs(branch);
  const tags = normalizeGlobs(tag);
  if ((branches === undefined) === (tags === undefined)) {
    throw new Error(
      'hm.push: pass exactly one of branch or tag\n  → e.g. push({ branch: "main" }) or push({ tag: "v*" })',
    );
  }
  return new PushTrigger(branches, tags);
}

const PR_TYPES = new Set([
  "opened",
  "synchronize",
  "reopened",
  "closed",
  "ready_for_review",
] as const);

type PrEventType = "opened" | "synchronize" | "reopened" | "closed" | "ready_for_review";

const DEFAULT_PR_TYPES: PrEventType[] = ["opened", "synchronize", "reopened"];

export class PullRequestTrigger {
  readonly branches: string[] | undefined;
  readonly types: string[];

  constructor(branches: string[] | undefined, types: string[]) {
    this.branches = branches;
    this.types = types;
  }

  toJSON(): Record<string, unknown> {
    const out: Record<string, unknown> = { event: "pull_request" };
    if (this.branches !== undefined) out.branches = this.branches;
    out.types = this.types;
    return out;
  }
}

export function pullRequest(opts?: {
  branches?: string | string[];
  types?: PrEventType[];
}): PullRequestTrigger {
  const types = opts?.types ?? DEFAULT_PR_TYPES;
  if (types.length === 0) {
    throw new Error("hm.pullRequest: types must be non-empty");
  }
  for (const t of types) {
    if (!PR_TYPES.has(t as any)) {
      const valid = [...PR_TYPES].sort().join(", ");
      throw new Error(`unknown pull_request type "${t}"\n  → valid: ${valid}`);
    }
  }
  return new PullRequestTrigger(normalizeGlobs(opts?.branches), [...types]);
}

export class ScheduleTrigger {
  readonly cron: string;

  constructor(cron: string) {
    this.cron = cron;
  }

  toJSON(): Record<string, unknown> {
    return { event: "schedule", cron: this.cron };
  }
}

const CRON_FIELD_RE = /^(\*|[0-9]+(-[0-9]+)?(\/[0-9]+)?|(\*\/[0-9]+))$/;

function isValidCron(expr: string): boolean {
  const fields = expr.trim().split(/\s+/);
  if (fields.length !== 5) return false;
  return fields.every((f) => {
    return f.split(",").every((part) => CRON_FIELD_RE.test(part));
  });
}

export function schedule(cron: string): ScheduleTrigger {
  if (!isValidCron(cron)) {
    throw new Error(
      `hm.schedule: invalid cron expression "${cron}"\n  → five-field crontab, UTC, e.g. '0 4 * * *'`,
    );
  }
  return new ScheduleTrigger(cron);
}
