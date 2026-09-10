// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Count Android-style unlock patterns on n-dimensional grids
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

//! Allowed node sets for successive visits of a pattern.

use serde::Serialize;

use crate::mask::{MAX_POINTS, Mask};

/// Validated node sets, indexed by visit. Node indices are zero-based.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct VisitFilters {
    #[serde(skip)]
    node_count: usize,
    allowed: Vec<Vec<usize>>,
}

impl VisitFilters {
    /// Validates the sets and removes duplicate indices without changing their meaning.
    ///
    /// # Errors
    /// Rejects unsupported node counts, more visits than nodes, or an index
    /// outside the grid. Empty sets and an empty list of visits are valid.
    pub fn new(node_count: usize, mut allowed: Vec<Vec<usize>>) -> Result<Self, String> {
        if node_count > MAX_POINTS {
            return Err(format!(
                "node count ({node_count}) exceeds the supported maximum of {MAX_POINTS}"
            ));
        }
        if allowed.len() > node_count {
            return Err(format!(
                "number of visits ({}) exceeds the number of points ({node_count})",
                allowed.len(),
            ));
        }
        for (visit, nodes) in allowed.iter_mut().enumerate() {
            if let Some(&node) = nodes.iter().find(|&&node| node >= node_count) {
                return Err(format!(
                    "visit {} contains node {node}, but the grid has {node_count} points",
                    visit + 1,
                ));
            }
            nodes.sort_unstable();
            nodes.dedup();
        }
        Ok(Self {
            node_count,
            allowed,
        })
    }

    /// Number of visits described by the filters.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.allowed.len()
    }

    /// Whether the filters describe only the empty pattern.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }

    /// Sorted, unique node indices for each visit.
    #[must_use]
    pub fn allowed(&self) -> &[Vec<usize>] {
        &self.allowed
    }

    /// Converts the validated sets to the chosen visited-mask width.
    ///
    /// # Panics
    /// Panics if the mask width cannot represent the grid's node count.
    #[must_use]
    pub fn masks<M: Mask>(&self) -> Vec<M> {
        assert!(
            self.node_count <= M::MAX_POINTS,
            "mask width is too small for the grid"
        );
        self.allowed
            .iter()
            .map(|nodes| {
                nodes
                    .iter()
                    .fold(M::ZERO, |mask, &node| mask | M::bit(node))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_normalize_and_serialize_as_the_input_shape() -> Result<(), Box<dyn std::error::Error>> {
        let filters = VisitFilters::new(4, vec![vec![3, 1, 3], vec![], vec![0, 2]])?;
        assert_eq!(filters.len(), 3);
        assert!(!filters.is_empty());
        assert_eq!(filters.allowed(), [vec![1, 3], vec![], vec![0, 2]]);
        assert_eq!(filters.masks::<u32>(), [0b1010, 0, 0b0101]);
        assert_eq!(serde_json::to_string(&filters)?, "[[1,3],[],[0,2]]");
        assert!(VisitFilters::new(0, vec![])?.is_empty());
        Ok(())
    }

    #[test]
    fn invalid_grid_indices_and_visit_counts_are_rejected() {
        assert!(VisitFilters::new(MAX_POINTS + 1, vec![]).is_err());
        assert!(VisitFilters::new(2, vec![vec![]; 3]).is_err());
        assert!(VisitFilters::new(2, vec![vec![2]]).is_err());
        assert!(VisitFilters::new(2, vec![vec![usize::MAX]]).is_err());
        assert!(VisitFilters::new(0, vec![vec![]]).is_err());
    }
}
