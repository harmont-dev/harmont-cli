import type { Step, StepOptions } from "../step.js";
import { forever, onChange } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const APT_PACKAGES = [
  "php-cli",
  "php-mbstring",
  "php-xml",
  "php-curl",
  "php-sqlite3",
  "composer",
  "git",
  "unzip",
] as const;

export interface ComposerOptions {
  readonly path?: string;
  readonly laravel?: boolean;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class ComposerProject {
  readonly path: string;
  private readonly _installed: Step;
  private readonly _tag: string;
  private readonly _laravel: boolean;

  constructor(
    path: string,
    installed: Step,
    tag: string,
    laravel: boolean,
  ) {
    this.path = path;
    this._installed = installed;
    this._tag = tag;
    this._laravel = laravel;
  }

  install(): Step {
    return this._installed;
  }

  test(opts?: ActionOptions): Step {
    const cmd = this._laravel
      ? `cd ${this.path} && php artisan test`
      : `cd ${this.path} && vendor/bin/phpunit`;
    return this._installed.sh(cmd, {
      label: `:${this._tag}: test`,
      ...opts,
    });
  }

  lint(opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && vendor/bin/phpstan analyse`,
      { label: `:${this._tag}: lint`, ...opts },
    );
  }
}

export function composer(opts?: ComposerOptions): ComposerProject {
  const path = opts?.path ?? ".";
  const laravel = opts?.laravel ?? false;
  const tag = laravel ? "laravel" : "php";

  const composerVerified = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd: "composer --version && php --version",
    installCache: forever(),
    langTag: tag,
    installTag: "composer",
    image: opts?.image,
    base: opts?.base,
  });

  const deps = composerVerified.sh(
    `cd ${path} && composer install --no-interaction --prefer-dist`,
    {
      label: `:${tag}: deps`,
      cache: onChange(`${path}/composer.lock`),
    },
  );

  return new ComposerProject(path, deps, tag, laravel);
}
