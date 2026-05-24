use std::collections::BTreeMap;

use daggy::petgraph::visit::IntoNodeReferences;
use daggy::{Dag, NodeIndex, Walker};

use schemars::JsonSchema as DeriveJsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct CommandStep {
    pub key: String,
    #[serde(default)]
    pub label: Option<String>,
    pub cmd: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
    #[serde(default)]
    pub cache: Option<Cache>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct Cache {
    pub policy: String,
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub step: CommandStep,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    BuildsIn,
    DependsOn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineGraph {
    #[serde(default = "default_version")]
    version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_image: Option<String>,
    #[serde(rename = "graph")]
    dag: Dag<Transition, EdgeKind>,
}

fn default_version() -> String {
    "0".to_string()
}

impl PipelineGraph {
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.dag.node_count()
    }

    #[must_use]
    pub fn default_image(&self) -> Option<&str> {
        self.default_image.as_deref()
    }

    #[must_use]
    pub fn get_transition(&self, idx: NodeIndex) -> &Transition {
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
        let mut indices: Vec<NodeIndex> = self.dag.graph().node_indices().collect();
        indices.sort();
        indices
            .into_iter()
            .filter(|&n| !self.is_chain_step(n))
            .map(|root| {
                std::iter::successors(Some(root), |&cur| {
                    self.builds_in_children(cur)
                        .into_iter()
                        .find(|&c| self.is_chain_step(c))
                })
                .collect()
            })
            .collect()
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
}
