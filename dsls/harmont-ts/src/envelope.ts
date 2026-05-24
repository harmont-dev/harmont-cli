import type { PipelineIR } from "./pipeline.js";
import type { Trigger } from "./triggers.js";

export interface PipelineDefinition {
  readonly slug: string;
  readonly name?: string;
  readonly allowManual?: boolean;
  readonly triggers?: readonly Trigger[];
  readonly pipeline: PipelineIR;
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

export function renderEnvelope(definitions: readonly PipelineDefinition[]): string {
  const envelope: EnvelopeJSON = {
    schema_version: "1",
    pipelines: definitions.map((def) => ({
      slug: def.slug,
      name: def.name ?? def.slug,
      allow_manual: def.allowManual ?? true,
      triggers: (def.triggers ?? []).map((t) => t.toJSON()),
      definition: def.pipeline,
    })),
  };
  return JSON.stringify(envelope);
}
