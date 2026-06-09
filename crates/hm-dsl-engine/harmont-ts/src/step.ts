import type { CachePolicy } from "./cache.js";
import { parseDuration } from "./duration.js";

export interface StepOptions {
  readonly label?: string;
  readonly cache?: CachePolicy;
  readonly env?: Readonly<Record<string, string>>;
  readonly image?: string;
  readonly runner?: string;
  readonly runnerArgs?: Readonly<Record<string, unknown>>;
  readonly key?: string;
  readonly cwd?: string;
}

let nextId = 0;

export class Step {
  readonly _id: number;
  readonly _cmd: string | null;
  readonly _parent: Step | null;
  readonly _isWait: boolean;
  readonly _continueOnFailure: boolean;
  readonly _label: string | undefined;
  readonly _cache: CachePolicy | undefined;
  readonly _env: Readonly<Record<string, string>> | undefined;
  readonly _timeoutSeconds: number | undefined;
  readonly _image: string | undefined;
  readonly _runner: string | undefined;
  readonly _runnerArgs: Readonly<Record<string, unknown>> | undefined;
  readonly _keyOverride: string | undefined;

  /** @internal */
  constructor(init: {
    cmd: string | null;
    parent: Step | null;
    isWait?: boolean;
    continueOnFailure?: boolean;
    label?: string;
    cache?: CachePolicy;
    env?: Record<string, string>;
    timeoutSeconds?: number;
    image?: string;
    runner?: string;
    runnerArgs?: Record<string, unknown>;
    keyOverride?: string;
  }) {
    this._id = nextId++;
    this._cmd = init.cmd;
    this._parent = init.parent;
    this._isWait = init.isWait ?? false;
    this._continueOnFailure = init.continueOnFailure ?? false;
    this._label = init.label;
    this._cache = init.cache;
    this._env = init.env;
    this._timeoutSeconds = init.timeoutSeconds;
    this._image = init.image;
    this._runner = init.runner;
    this._runnerArgs = init.runnerArgs;
    this._keyOverride = init.keyOverride;
  }

  sh(cmd: string, opts?: StepOptions): Step {
    if (opts?.cwd === "") {
      throw new Error(
        'hm: cwd must be a non-empty path\n  → omit cwd to run in the workspace root, or pass cwd="some/dir"',
      );
    }
    const effectiveCmd = opts?.cwd != null ? `cd ${opts.cwd} && ${cmd}` : cmd;
    const effectiveImage =
      opts?.image != null
        ? opts.image
        : this._cmd === null
          ? this._image
          : undefined;
    return new Step({
      cmd: effectiveCmd,
      parent: this,
      label: opts?.label,
      cache: opts?.cache,
      env: opts?.env,
      image: effectiveImage,
      runner: opts?.runner,
      runnerArgs: opts?.runnerArgs,
      keyOverride: opts?.key,
    });
  }

  fork(opts?: { label?: string }): Step {
    return new Step({
      cmd: null,
      parent: this,
      label: opts?.label,
    });
  }

  /** @internal — returns a copy with the timeout set; preserves the chain. */
  withTimeoutSeconds(seconds: number): Step {
    return new Step({
      cmd: this._cmd,
      parent: this._parent,
      isWait: this._isWait,
      continueOnFailure: this._continueOnFailure,
      label: this._label,
      cache: this._cache,
      env: this._env as Record<string, string> | undefined,
      timeoutSeconds: seconds,
      image: this._image,
      runner: this._runner,
      runnerArgs: this._runnerArgs as Record<string, unknown> | undefined,
      keyOverride: this._keyOverride,
    });
  }
}

export function scratch(opts?: { image?: string }): Step {
  return new Step({ cmd: null, parent: null, image: opts?.image });
}

export function sh(cmd: string, opts?: StepOptions): Step {
  return scratch().sh(cmd, opts);
}

export function wait(opts?: { continueOnFailure?: boolean }): Step {
  return new Step({
    cmd: null,
    parent: null,
    isWait: true,
    continueOnFailure: opts?.continueOnFailure ?? false,
  });
}

/**
 * Apply a wall-clock timeout to a single step. The executor (and `hm run`
 * locally) kills the step once `duration` elapses; the step then fails as
 * timed out. Wrapping a step that already has a timeout replaces it.
 *
 * @param duration "30s" / "5m" / "1h30m" (units h, m, s) or a number of seconds.
 * @param step     A command step (not a `wait` barrier).
 */
export function timeout(duration: string | number, step: Step): Step {
  if (step._isWait) {
    throw new Error(
      'hm: timeout() cannot wrap a wait() barrier\n' +
        '  → apply timeout() to a command step, e.g. timeout("30s", sh("make test"))',
    );
  }
  return step.withTimeoutSeconds(parseDuration(duration));
}
