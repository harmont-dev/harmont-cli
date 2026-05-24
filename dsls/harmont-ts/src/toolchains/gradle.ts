import type { Step, StepOptions } from "../step.js";
import { forever } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const GRADLE_VERSION = "8.10";
const JDK_RE = /^(11|17|21)$/;

export interface GradleOptions {
  readonly path?: string;
  readonly jdk?: string;
  readonly kotlin?: boolean;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class GradleProject {
  readonly path: string;
  private readonly _installed: Step;
  private readonly _tag: string;

  constructor(path: string, installed: Step, tag: string) {
    this.path = path;
    this._installed = installed;
    this._tag = tag;
  }

  install(): Step {
    return this._installed;
  }

  build(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && gradle build`, {
      label: `:${this._tag}: build`,
      ...opts,
    });
  }

  test(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && gradle test`, {
      label: `:${this._tag}: test`,
      ...opts,
    });
  }

  lint(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && gradle check`, {
      label: `:${this._tag}: lint`,
      ...opts,
    });
  }
}

export function gradle(opts?: GradleOptions): GradleProject {
  const path = opts?.path ?? ".";
  const jdk = opts?.jdk ?? "21";
  const kotlin = opts?.kotlin ?? false;
  const tag = kotlin ? "kotlin" : "java";

  if (!JDK_RE.test(jdk)) {
    throw new Error(
      `hm.gradle: invalid jdk "${jdk}"\n  → use "11", "17", or "21"`,
    );
  }

  const aptPackages = [
    "curl",
    "ca-certificates",
    "unzip",
    `openjdk-${jdk}-jdk-headless`,
  ];

  const installCmd = [
    `curl -fsSL https://services.gradle.org/distributions/gradle-${GRADLE_VERSION}-bin.zip -o /tmp/gradle.zip`,
    "unzip -q /tmp/gradle.zip -d /opt",
    `ln -sf /opt/gradle-${GRADLE_VERSION}/bin/gradle /usr/local/bin/gradle`,
    "rm /tmp/gradle.zip",
    "java -version && gradle --version",
  ].join(" && ");

  const installed = makeInstallChain({
    aptPackages,
    installCmd,
    installCache: forever(),
    langTag: tag,
    installTag: "jdk",
    image: opts?.image,
    base: opts?.base,
  });

  return new GradleProject(path, installed, tag);
}
