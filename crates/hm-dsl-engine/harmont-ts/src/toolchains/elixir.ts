import type { Step, StepOptions } from "../step.js";
import { forever, onChange } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const APT_PACKAGES = [
  "curl",
  "ca-certificates",
  "git",
  "unzip",
  "build-essential",
  "autoconf",
  "libncurses-dev",
  "libssl-dev",
] as const;

const ELIXIR_ENV = { ELIXIR_ERL_OPTIONS: "+fnu" } as const;

const ELIXIR_VERSION_RE = /^[0-9]+\.[0-9]+\.[0-9]+$/;
const OTP_VERSION_RE = /^[0-9]+(\.[0-9]+(\.[0-9]+)?)?$/;

export interface ElixirOptions {
  readonly path?: string;
  readonly elixirVersion?: string;
  readonly otpVersion?: string;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class ElixirProject {
  readonly path: string;
  private readonly _installed: Step;
  private _plt: Step | null = null;

  constructor(path: string, installed: Step) {
    this.path = path;
    this._installed = installed;
  }

  install(): Step {
    return this._installed;
  }

  /** Append a post-install command and return an advanced project; chainable.
   *  For prep steps the toolchain's actions must depend on but the SDK does not
   *  model natively (codegen, fixtures, extra tooling). Action methods on the
   *  returned object fork from this step.
   *  @example hm.elixir({ path: "elixir" }).setup("mix proto.gen").compile() */
  setup(cmd: string, opts?: StepOptions): ElixirProject {
    return new ElixirProject(this.path, this._installed.sh(cmd, opts));
  }

  private _sh(parent: Step, cmd: string, opts?: ActionOptions): Step {
    const { env: userEnv, ...rest } = opts ?? {};
    return parent.sh(cmd, { env: { ...ELIXIR_ENV, ...userEnv }, ...rest });
  }

  compile(opts?: ActionOptions): Step {
    return this._sh(this._installed,
      `cd ${this.path} && mix compile --warnings-as-errors`,
      { label: ":ex: compile", ...opts },
    );
  }

  test(opts?: ActionOptions & { cover?: boolean; partitions?: number }): Step {
    const flags: string[] = [];
    if (opts?.cover) flags.push("--cover");
    if (opts?.partitions != null) flags.push(`--partitions ${opts.partitions}`);
    const { cover: _, partitions: __, ...rest } = opts ?? {};
    const flagStr = flags.length > 0 ? ` ${flags.join(" ")}` : "";
    return this._sh(this._installed, `cd ${this.path} && mix test${flagStr}`, {
      label: ":ex: test",
      ...rest,
    });
  }

  format(opts?: ActionOptions): Step {
    return this._sh(this._installed,
      `cd ${this.path} && mix format --check-formatted`,
      { label: ":ex: format", ...opts },
    );
  }

  credo(opts?: ActionOptions & { strict?: boolean }): Step {
    const strict = opts?.strict !== false ? " --strict" : "";
    const { strict: _, ...rest } = opts ?? {};
    return this._sh(this._installed, `cd ${this.path} && mix credo${strict}`, {
      label: ":ex: credo",
      ...rest,
    });
  }

  plt(): Step {
    if (this._plt == null) {
      this._plt = this._sh(this._installed, `cd ${this.path} && mix dialyzer --plt`, {
        label: ":ex: plt",
        cache: onChange(`${this.path}/mix.lock`),
      });
    }
    return this._plt;
  }

  dialyzer(opts?: ActionOptions): Step {
    return this._sh(this.plt(), `cd ${this.path} && mix dialyzer`, {
      label: ":ex: dialyzer",
      ...opts,
    });
  }

  sobelow(opts?: ActionOptions): Step {
    return this._sh(this._installed, `cd ${this.path} && mix sobelow --exit`, {
      label: ":ex: sobelow",
      ...opts,
    });
  }

  depsAudit(opts?: ActionOptions): Step {
    return this._sh(this._installed, `cd ${this.path} && mix deps.audit`, {
      label: ":ex: deps-audit",
      ...opts,
    });
  }

  hexAudit(opts?: ActionOptions): Step {
    return this._sh(this._installed, `cd ${this.path} && mix hex.audit`, {
      label: ":ex: hex-audit",
      ...opts,
    });
  }

  mix(task: string, opts?: ActionOptions): Step {
    return this._sh(this._installed, `cd ${this.path} && mix ${task}`, {
      label: `:ex: ${task}`,
      ...opts,
    });
  }

  release(opts?: ActionOptions & { mixEnv?: string }): Step {
    const env = opts?.mixEnv ?? "prod";
    const { mixEnv: _, ...rest } = opts ?? {};
    return this._sh(this._installed,
      `cd ${this.path} && MIX_ENV=${env} mix release`,
      { label: ":ex: release", ...rest },
    );
  }

}

export function elixir(opts?: ElixirOptions): ElixirProject {
  const path = opts?.path ?? ".";
  const elixirVersion = opts?.elixirVersion ?? "1.18.3";
  const otpVersion = opts?.otpVersion ?? "27.3.3";

  if (!ELIXIR_VERSION_RE.test(elixirVersion)) {
    throw new Error(
      `hm.elixir: invalid elixir version "${elixirVersion}"\n  → use a semver like "1.18.3"`,
    );
  }

  if (!OTP_VERSION_RE.test(otpVersion)) {
    throw new Error(
      `hm.elixir: invalid otp version "${otpVersion}"\n  → use a semver like "27" or "27.3.3"`,
    );
  }

  const otpMajor = otpVersion.split(".")[0];

  const erlangInstallCmd = [
    `curl -fsSL https://binaries2.erlang-solutions.com/debian/pool/contrib/e/esl-erlang/esl-erlang_${otpVersion}-1~debian~bookworm_amd64.deb -o /tmp/erlang.deb`,
    "(dpkg -i /tmp/erlang.deb || apt-get install -fy)",
    "erl -eval 'erlang:display(erlang:system_info(otp_release)), halt().' -noshell",
  ].join(" && ");

  const erlangInstalled = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd: erlangInstallCmd,
    installCache: forever(),
    langTag: "ex",
    installTag: "erlang-install",
    image: opts?.image,
    base: opts?.base,
  });

  const elixirInstalled = erlangInstalled.sh(
    [
      `curl -fsSL https://github.com/elixir-lang/elixir/releases/download/v${elixirVersion}/elixir-otp-${otpMajor}.zip -o /tmp/elixir.zip`,
      "unzip -q /tmp/elixir.zip -d /usr/local/elixir",
      "ln -sf /usr/local/elixir/bin/elixir /usr/local/bin/elixir",
      "ln -sf /usr/local/elixir/bin/mix /usr/local/bin/mix",
      "ln -sf /usr/local/elixir/bin/iex /usr/local/bin/iex",
      "mix local.hex --force",
      "mix local.rebar --force",
      "elixir --version",
    ].join(" && "),
    { label: ":ex: elixir-install", cache: forever(), env: ELIXIR_ENV },
  );

  const depsInstalled = elixirInstalled.sh(
    `cd ${path} && mix deps.get && mix deps.compile`,
    {
      label: ":ex: mix-deps",
      cache: onChange(`${path}/mix.lock`),
      env: ELIXIR_ENV,
    },
  );

  return new ElixirProject(path, depsInstalled);
}
