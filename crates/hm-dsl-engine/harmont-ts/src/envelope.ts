import { resolvePipelineCacheKeys } from "./keygen.js";
import type { PipelineIR } from "./pipeline.js";
import type { Trigger } from "./triggers.js";

export interface PipelineDefinition {
  readonly slug: string;
  readonly name?: string;
  readonly allowManual?: boolean;
  readonly triggers?: readonly Trigger[];
  readonly pipeline: PipelineIR;
}

export interface RenderOptions {
  readonly basePath?: string;
  readonly pipelineOrg?: string;
  readonly now?: number;
  readonly env?: Readonly<Record<string, string>>;
}

interface EnvelopeJSON {
  schema_version: string;
  pipelines: EnvelopePipelineJSON[];
}

interface EnvelopePipelineJSON {
  slug: string;
  name: string;
  allow_manual: boolean;
  triggers: Record<string, unknown>[];
  definition: PipelineIR;
}

export function renderEnvelope(
  definitions: readonly PipelineDefinition[],
  opts?: RenderOptions,
): string {
  const pipelineOrg =
    opts?.pipelineOrg ??
    (typeof process !== "undefined"
      ? process.env.HARMONT_PIPELINE_ORG
      : undefined) ??
    "default";
  const now = opts?.now ?? Math.floor(Date.now() / 1000);
  const basePath = opts?.basePath;
  const env: Readonly<Record<string, string>> =
    opts?.env ??
    (typeof process !== "undefined"
      ? (process.env as Record<string, string>)
      : {});

  const envelope: EnvelopeJSON = {
    schema_version: "1",
    pipelines: definitions.map((def) => {
      const entry: EnvelopePipelineJSON = {
        slug: def.slug,
        name: def.name ?? def.slug,
        allow_manual: def.allowManual ?? true,
        triggers: (def.triggers ?? []).map((t) => t.toJSON()),
        definition: def.pipeline,
      };

      if (basePath != null) {
        resolvePipelineCacheKeys(entry.definition.graph, {
          pipelineOrg,
          pipelineSlug: def.slug,
          now,
          basePath,
          env,
        });
      }

      return entry;
    }),
  };
  return JSON.stringify(envelope);
}
