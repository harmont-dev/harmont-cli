use std::collections::BTreeMap;

use anyhow::{Context, Result};
use daggy::petgraph::visit::IntoNodeReferences;
use daggy::{Dag, NodeIndex, Walker};

use crate::{CommandStep, Pipeline, Step};

#[derive(Debug, Clone)]
pub struct NodeWeight {
    pub step: CommandStep,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    BuildsIn,
    DependsOn,
}

#[derive(Debug, Clone)]
pub struct PipelineGraph {
    dag: Dag<NodeWeight, EdgeKind>,
    default_image: Option<String>,
}

struct FlatStep {
    step: CommandStep,
    extra_deps: Vec<String>,
}

impl PipelineGraph {
    pub fn build(pipeline: &Pipeline) -> Result<Self> {
        let flat = flatten_steps(&pipeline.steps);
        let pipeline_env = pipeline.env.clone().unwrap_or_default();

        let mut dag: Dag<NodeWeight, EdgeKind> = Dag::new();
        let mut key_to_idx: BTreeMap<String, NodeIndex> = BTreeMap::new();

        for f in &flat {
            let mut env = pipeline_env.clone();
            if let Some(e) = &f.step.env {
                env.extend(e.clone());
            }
            let idx = dag.add_node(NodeWeight {
                step: f.step.clone(),
                env,
            });
            key_to_idx.insert(f.step.key.clone(), idx);
        }

        for f in &flat {
            let child = key_to_idx[&f.step.key];

            if let Some(parent_key) = &f.step.builds_in {
                let parent = *key_to_idx.get(parent_key).ok_or_else(|| {
                    anyhow::anyhow!(
                        "step '{}' builds_in references unknown step '{}'",
                        f.step.key,
                        parent_key
                    )
                })?;
                dag.add_edge(parent, child, EdgeKind::BuildsIn)
                    .context("cycle detected adding builds_in edge")?;
            }

            for dep_key in &f.extra_deps {
                let parent = *key_to_idx.get(dep_key).ok_or_else(|| {
                    anyhow::anyhow!(
                        "step '{}' has wait-barrier dep on unknown step '{}'",
                        f.step.key,
                        dep_key
                    )
                })?;
                if f.step.builds_in.as_deref() == Some(dep_key) {
                    continue;
                }
                dag.add_edge(parent, child, EdgeKind::DependsOn)
                    .context("cycle detected adding wait-barrier edge")?;
            }
        }

        if let Some(default_img) = pipeline.default_image.as_deref() {
            for idx in dag.graph().node_indices() {
                let has_builds_in_parent = dag
                    .parents(idx)
                    .iter(&dag)
                    .any(|(e, _)| dag.edge_weight(e).copied() == Some(EdgeKind::BuildsIn));
                if !has_builds_in_parent {
                    if let Some(w) = dag.node_weight_mut(idx) {
                        if w.step.image.is_none() {
                            w.step.image = Some(default_img.to_string());
                        }
                    }
                }
            }
        }

        Ok(Self {
            dag,
            default_image: pipeline.default_image.clone(),
        })
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.dag.node_count()
    }

    #[must_use]
    pub fn default_image(&self) -> Option<&str> {
        self.default_image.as_deref()
    }

    #[must_use]
    pub fn node_weight(&self, idx: NodeIndex) -> &NodeWeight {
        &self.dag[idx]
    }

    #[must_use]
    pub fn node_index_by_key(&self, key: &str) -> Option<NodeIndex> {
        self.dag
            .graph()
            .node_references()
            .find(|(_, w)| w.step.key == key)
            .map(|(idx, _)| idx)
    }

    #[must_use]
    pub fn parent_keys(&self, idx: NodeIndex) -> Vec<String> {
        self.dag
            .parents(idx)
            .iter(&self.dag)
            .map(|(_, parent_idx)| self.dag[parent_idx].step.key.clone())
            .collect()
    }

    #[must_use]
    pub fn builds_in_parent(&self, idx: NodeIndex) -> Option<NodeIndex> {
        self.dag
            .parents(idx)
            .iter(&self.dag)
            .find(|(e, _)| self.dag.edge_weight(*e).copied() == Some(EdgeKind::BuildsIn))
            .map(|(_, parent_idx)| parent_idx)
    }

    #[must_use]
    pub fn builds_in_children(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        self.dag
            .children(idx)
            .iter(&self.dag)
            .filter(|(e, _)| self.dag.edge_weight(*e).copied() == Some(EdgeKind::BuildsIn))
            .map(|(_, child_idx)| child_idx)
            .collect()
    }

    #[must_use]
    pub fn all_parents(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        self.dag
            .parents(idx)
            .iter(&self.dag)
            .map(|(_, parent_idx)| parent_idx)
            .collect()
    }

    #[must_use]
    pub fn is_chain_step(&self, idx: NodeIndex) -> bool {
        self.builds_in_parent(idx).is_some_and(|parent| {
            self.builds_in_children(parent).len() == 1 && self.all_parents(idx).len() == 1
        })
    }

    #[must_use]
    pub fn chains(&self) -> Vec<Vec<NodeIndex>> {
        let mut placed: BTreeMap<NodeIndex, bool> = BTreeMap::new();
        let mut out: Vec<Vec<NodeIndex>> = Vec::new();
        let mut indices: Vec<NodeIndex> = self.dag.graph().node_indices().collect();
        indices.sort();
        for root in &indices {
            if *placed.get(root).unwrap_or(&false) || self.is_chain_step(*root) {
                continue;
            }
            let mut chain = vec![*root];
            placed.insert(*root, true);
            let mut cur = *root;
            while let Some(next) = self
                .builds_in_children(cur)
                .into_iter()
                .find(|&c| self.is_chain_step(c))
            {
                chain.push(next);
                placed.insert(next, true);
                cur = next;
            }
            out.push(chain);
        }
        out
    }

    #[must_use]
    pub fn chain_deps(&self, chains: &[Vec<NodeIndex>]) -> Vec<Vec<usize>> {
        let mut chain_index: BTreeMap<NodeIndex, usize> = BTreeMap::new();
        for (ci, ch) in chains.iter().enumerate() {
            for &n in ch {
                chain_index.insert(n, ci);
            }
        }
        let mut out: Vec<Vec<usize>> = vec![Vec::new(); chains.len()];
        for (ci, ch) in chains.iter().enumerate() {
            let mut seen = std::collections::BTreeSet::new();
            for &n in ch {
                for parent in self.all_parents(n) {
                    let dep_ci = chain_index[&parent];
                    if dep_ci != ci {
                        seen.insert(dep_ci);
                    }
                }
            }
            out[ci] = seen.into_iter().collect();
        }
        out
    }

    pub fn node_indices(&self) -> impl Iterator<Item = NodeIndex> + '_ {
        self.dag.graph().node_indices()
    }
}

fn flatten_steps(steps: &[Step]) -> Vec<FlatStep> {
    let mut out: Vec<FlatStep> = Vec::new();
    let mut implicit_wait_targets: Vec<String> = Vec::new();
    for s in steps {
        match s {
            Step::Command(c) => {
                out.push(FlatStep {
                    step: (**c).clone(),
                    extra_deps: implicit_wait_targets.clone(),
                });
            }
            Step::Wait(_) => {
                implicit_wait_targets = out.iter().map(|f| f.step.key.clone()).collect();
            }
        }
    }
    out
}
