import type { CachePolicy } from "./cache.js";
import { resolveKeys } from "./keys.js";
import type { Step } from "./step.js";

export interface PipelineOptions {
  readonly env?: Readonly<Record<string, string>>;
  readonly defaultImage?: string;
}

export interface PipelineIR {
  version: string;
  default_image?: string;
  graph: {
    nodes: GraphNode[];
    node_holes: never[];
    edge_property: "directed";
    edges: [number, number, string][];
  };
}

interface GraphNode {
  step: Record<string, unknown>;
  env: Record<string, string>;
}

export function pipeline(...args: (Step | PipelineOptions)[]): PipelineIR {
  if (args.length === 0) {
    throw new Error(
      "pipeline must have at least one leaf — pass the terminal step(s) of each branch as positional args",
    );
  }

  let leaves: Step[];
  let opts: PipelineOptions | undefined;

  const last = args[args.length - 1];
  if (last && typeof last === "object" && !("_id" in last)) {
    opts = last as PipelineOptions;
    leaves = args.slice(0, -1) as Step[];
  } else {
    leaves = args as Step[];
  }

  if (leaves.length === 0) {
    throw new Error(
      "pipeline must have at least one leaf — pass the terminal step(s) of each branch as positional args",
    );
  }

  const ir: PipelineIR = { version: "0", graph: lowerToGraph(leaves, opts) };
  if (opts?.defaultImage != null) {
    ir.default_image = opts.defaultImage;
  }
  return ir;
}

function lowerToGraph(
  leaves: Step[],
  opts?: PipelineOptions,
): PipelineIR["graph"] {
  const ordered = topoCollect(leaves);
  const commandSteps = ordered.filter((s) => s._cmd !== null && !s._isWait);
  const keys = resolveKeys(commandSteps);

  const idxById = new Map<number, number>();
  for (let i = 0; i < commandSteps.length; i++) {
    idxById.set(commandSteps[i]._id, i);
  }

  const hasBuildsInParent = new Set<number>();
  const nodes: GraphNode[] = [];
  const edges: [number, number, string][] = [];

  let preWaitIndices: number[] = [];
  let pendingDependsOn: number[] = [];

  for (const s of ordered) {
    if (s._isWait) {
      pendingDependsOn = [...preWaitIndices];
      preWaitIndices = [];
      continue;
    }

    if (s._cmd === null) continue;

    const nodeIdx = idxById.get(s._id)!;
    const stepKey = keys.get(s._id)!;

    const stepDict: Record<string, unknown> = {
      key: stepKey,
      cmd: s._cmd,
    };
    if (s._label != null) stepDict.label = s._label;
    if (s._cache != null) stepDict.cache = cachePolicyToDict(s._cache);
    if (s._timeoutSeconds != null) stepDict.timeout_seconds = s._timeoutSeconds;
    if (s._image != null) stepDict.image = s._image;
    if (s._runner != null) stepDict.runner = s._runner;
    if (s._runnerArgs != null) stepDict.runner_args = s._runnerArgs;

    const mergedEnv: Record<string, string> = {};
    if (opts?.env) Object.assign(mergedEnv, opts.env);
    if (s._env) Object.assign(mergedEnv, s._env);

    nodes.push({ step: stepDict, env: mergedEnv });

    const parentKey = resolvedParentKey(s, keys);
    if (parentKey !== null) {
      const parentIdx = findIdxByKey(parentKey, commandSteps, keys, idxById);
      edges.push([parentIdx, nodeIdx, "builds_in"]);
      hasBuildsInParent.add(nodeIdx);
    }

    for (const depIdx of pendingDependsOn) {
      edges.push([depIdx, nodeIdx, "depends_on"]);
    }

    preWaitIndices.push(nodeIdx);
  }

  if (opts?.defaultImage != null) {
    for (let i = 0; i < nodes.length; i++) {
      if (!hasBuildsInParent.has(i) && !("image" in nodes[i].step)) {
        nodes[i].step.image = opts.defaultImage;
      }
    }
  }

  return {
    nodes,
    node_holes: [],
    edge_property: "directed",
    edges,
  };
}

function topoCollect(leaves: Step[]): Step[] {
  const seen = new Set<number>();
  const ordered: Step[] = [];

  for (const leaf of leaves) {
    if (leaf._isWait) {
      ordered.push(leaf);
      continue;
    }
    const chain: Step[] = [];
    let node: Step | null = leaf;
    while (node !== null) {
      if (seen.has(node._id)) break;
      chain.push(node);
      node = node._parent;
    }
    for (let i = chain.length - 1; i >= 0; i--) {
      const s = chain[i];
      if (seen.has(s._id)) continue;
      seen.add(s._id);
      ordered.push(s);
    }
  }

  return ordered;
}

function resolvedParentKey(
  s: Step,
  keys: Map<number, string>,
): string | null {
  let node = s._parent;
  while (node !== null) {
    if (node._cmd !== null && !node._isWait) {
      return keys.get(node._id) ?? null;
    }
    node = node._parent;
  }
  return null;
}

function findIdxByKey(
  key: string,
  commandSteps: Step[],
  keys: Map<number, string>,
  idxById: Map<number, number>,
): number {
  for (const s of commandSteps) {
    if (keys.get(s._id) === key) {
      return idxById.get(s._id)!;
    }
  }
  throw new Error(`BUG: no step with key "${key}"`);
}

function cachePolicyToDict(policy: CachePolicy): Record<string, unknown> {
  switch (policy.kind) {
    case "forever":
      return { policy: "forever", env_keys: [...policy.envKeys] };
    case "ttl":
      return {
        policy: "ttl",
        duration_seconds: policy.durationSeconds,
        env_keys: [...policy.envKeys],
      };
    case "on_change":
      return { policy: "on_change", paths: [...policy.paths] };
    case "compose":
      return {
        policy: "compose",
        sub_policies: policy.policies.map(cachePolicyToDict),
      };
  }
}
