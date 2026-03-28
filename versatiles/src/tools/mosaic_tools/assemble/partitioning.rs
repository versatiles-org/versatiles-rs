//! PCA-based recursive bisection for batch grouping.
//!
//! Tiles are collapsed into signature groups (tiles sharing the same source set),
//! then recursively bisected using PCA to find the most divisive axis at each level.
//! This minimizes `Σ |unique_sources(batch)|` — the total number of source-opens
//! across all batches.

use std::collections::{HashMap, HashSet};
use versatiles_core::TileCoord;

/// A group of tiles sharing the same source-set signature.
pub(super) struct SignatureGroup {
	pub sources: Vec<usize>,    // sorted source indices (the signature)
	pub coords: Vec<TileCoord>, // all tiles with this signature
}

/// Collapse a translucent map into signature groups by grouping on the source list.
pub(super) fn collapse_into_signature_groups(translucent_map: HashMap<TileCoord, Vec<usize>>) -> Vec<SignatureGroup> {
	let mut sig_map: HashMap<Vec<usize>, Vec<TileCoord>> = HashMap::new();
	for (coord, mut sources) in translucent_map {
		sources.sort_unstable();
		sig_map.entry(sources).or_default().push(coord);
	}
	sig_map
		.into_iter()
		.map(|(sources, coords)| SignatureGroup { sources, coords })
		.collect()
}

/// Recursively partition signature groups into batches of at most `batch_size` tiles.
pub(super) fn partition_into_batches(
	groups: Vec<SignatureGroup>,
	num_sources: usize,
	batch_size: usize,
) -> Vec<Vec<SignatureGroup>> {
	let total_tiles: usize = groups.iter().map(|g| g.coords.len()).sum();

	// Base case: fits in one batch
	if total_tiles <= batch_size {
		return vec![groups];
	}

	// Special case: single group that exceeds batch_size — split coords spatially
	if groups.len() == 1 {
		return split_single_group(groups.into_iter().next().unwrap(), batch_size);
	}

	// PCA step: find the principal component via power iteration
	let (left, right) = pca_bisect(groups, num_sources);

	// Recurse on each half
	let mut batches = partition_into_batches(left, num_sources, batch_size);
	batches.extend(partition_into_batches(right, num_sources, batch_size));
	batches
}

/// Split a single oversized group into spatial chunks.
fn split_single_group(g: SignatureGroup, batch_size: usize) -> Vec<Vec<SignatureGroup>> {
	let mut coords = g.coords;
	coords.sort_by(|a, b| a.level.cmp(&b.level).then(a.x.cmp(&b.x)).then(a.y.cmp(&b.y)));
	coords
		.chunks(batch_size)
		.map(|chunk| {
			vec![SignatureGroup {
				sources: g.sources.clone(),
				coords: chunk.to_vec(),
			}]
		})
		.collect()
}

/// Bisect groups using PCA projection onto the first principal component.
///
/// Returns (left_half, right_half) split at the median tile count.
fn pca_bisect(groups: Vec<SignatureGroup>, num_sources: usize) -> (Vec<SignatureGroup>, Vec<SignatureGroup>) {
	let total_tiles: usize = groups.iter().map(|g| g.coords.len()).sum();

	// Build source membership sets for efficient lookup
	let source_sets: Vec<HashSet<usize>> = groups.iter().map(|g| g.sources.iter().copied().collect()).collect();

	// Compute weighted mean
	let total_weight: f64 = groups.iter().map(|g| g.coords.len() as f64).sum();
	let mut mean = vec![0.0f64; num_sources];
	for (i, g) in groups.iter().enumerate() {
		let w = g.coords.len() as f64;
		for (s, m) in mean.iter_mut().enumerate() {
			if source_sets[i].contains(&s) {
				*m += w;
			}
		}
	}
	for m in &mut mean {
		*m /= total_weight;
	}

	// Power iteration to find PC1
	let mut v = initial_vector(num_sources);

	for _ in 0..15 {
		power_iteration_step(&groups, &source_sets, &mean, &mut v, num_sources);
	}

	// Project each group onto PC1 and sort by score
	let mut scored: Vec<(f64, usize)> = groups
		.iter()
		.enumerate()
		.map(|(i, _)| {
			let score: f64 = (0..num_sources)
				.map(|s| {
					let x = if source_sets[i].contains(&s) { 1.0 } else { 0.0 };
					(x - mean[s]) * v[s]
				})
				.sum();
			(score, i)
		})
		.collect();
	scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

	// Find the split point: balanced by tile count
	let half = total_tiles / 2;
	let mut acc = 0usize;
	let mut split_pos = 1;
	for (pos, &(_, idx)) in scored.iter().enumerate() {
		acc += groups[idx].coords.len();
		if acc >= half {
			split_pos = pos + 1;
			break;
		}
	}
	split_pos = split_pos.clamp(1, scored.len() - 1);

	// Split into two halves
	let indices_left: Vec<usize> = scored[..split_pos].iter().map(|&(_, i)| i).collect();
	let indices_right: Vec<usize> = scored[split_pos..].iter().map(|&(_, i)| i).collect();

	let mut groups_by_idx: Vec<Option<SignatureGroup>> = groups.into_iter().map(Some).collect();

	let left: Vec<SignatureGroup> = indices_left
		.into_iter()
		.map(|i| groups_by_idx[i].take().unwrap())
		.collect();
	let right: Vec<SignatureGroup> = indices_right
		.into_iter()
		.map(|i| groups_by_idx[i].take().unwrap())
		.collect();

	(left, right)
}

/// Create a normalized initial vector for power iteration.
fn initial_vector(num_sources: usize) -> Vec<f64> {
	let mut v: Vec<f64> = (1..=num_sources).map(|i| i as f64).collect();
	let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
	for x in &mut v {
		*x /= norm;
	}
	v
}

/// One iteration of the power method, applied implicitly (no matrix allocation).
fn power_iteration_step(
	groups: &[SignatureGroup],
	source_sets: &[HashSet<usize>],
	mean: &[f64],
	v: &mut Vec<f64>,
	num_sources: usize,
) {
	let mut new_v = vec![0.0f64; num_sources];
	for (i, g) in groups.iter().enumerate() {
		let w = g.coords.len() as f64;
		// dot = (x_i - μ) · v
		let dot: f64 = (0..num_sources)
			.map(|s| {
				let x = if source_sets[i].contains(&s) { 1.0 } else { 0.0 };
				(x - mean[s]) * v[s]
			})
			.sum();
		// accumulate: new_v += w * dot * (x_i - μ)
		for s in 0..num_sources {
			let x = if source_sets[i].contains(&s) { 1.0 } else { 0.0 };
			new_v[s] += w * dot * (x - mean[s]);
		}
	}
	let n = new_v.iter().map(|x| x * x).sum::<f64>().sqrt();
	if n > 0.0 {
		for x in &mut new_v {
			*x /= n;
		}
	}
	*v = new_v;
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::BTreeSet;

	fn tc(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord { level, x, y }
	}

	#[test]
	fn test_collapse_into_signature_groups() {
		let mut map: HashMap<TileCoord, Vec<usize>> = HashMap::new();
		map.insert(tc(0, 0, 0), vec![1, 0]);
		map.insert(tc(0, 1, 0), vec![0, 1]);
		map.insert(tc(0, 2, 0), vec![2, 3]);

		let groups = collapse_into_signature_groups(map);
		// Two unique signatures: [0,1] and [2,3]
		assert_eq!(groups.len(), 2);

		for g in &groups {
			if g.sources == vec![0, 1] {
				assert_eq!(g.coords.len(), 2);
			} else if g.sources == vec![2, 3] {
				assert_eq!(g.coords.len(), 1);
			} else {
				panic!("unexpected signature: {:?}", g.sources);
			}
		}
	}

	#[test]
	fn test_partition_single_group_over_batch_size() {
		let coords: Vec<TileCoord> = (0..10).map(|i| tc(0, i, 0)).collect();
		let groups = vec![SignatureGroup {
			sources: vec![0, 1],
			coords,
		}];

		let batches = partition_into_batches(groups, 4, 3);
		// 10 tiles, batch_size=3 → 4 batches (3+3+3+1)
		assert_eq!(batches.len(), 4);

		let total: usize = batches.iter().flat_map(|b| b.iter()).map(|g| g.coords.len()).sum();
		assert_eq!(total, 10);

		for batch in &batches {
			let batch_tiles: usize = batch.iter().map(|g| g.coords.len()).sum();
			assert!(batch_tiles <= 3);
		}
	}

	#[test]
	fn test_partition_separates_disjoint_sources() {
		let groups = vec![
			SignatureGroup {
				sources: vec![0, 1],
				coords: vec![tc(0, 0, 0), tc(0, 1, 0), tc(0, 2, 0)],
			},
			SignatureGroup {
				sources: vec![2, 3],
				coords: vec![tc(1, 0, 0), tc(1, 1, 0), tc(1, 2, 0)],
			},
		];

		let batches = partition_into_batches(groups, 4, 3);
		assert_eq!(batches.len(), 2);

		for batch in &batches {
			let batch_tiles: usize = batch.iter().map(|g| g.coords.len()).sum();
			assert_eq!(batch_tiles, 3);
			let all_sources: BTreeSet<usize> = batch.iter().flat_map(|g| g.sources.iter().copied()).collect();
			assert!(
				all_sources == [0, 1].into_iter().collect::<BTreeSet<_>>()
					|| all_sources == [2, 3].into_iter().collect::<BTreeSet<_>>()
			);
		}
	}

	#[test]
	fn test_partition_merges_identical_sources() {
		let groups = vec![
			SignatureGroup {
				sources: vec![0, 1],
				coords: vec![tc(0, 0, 0)],
			},
			SignatureGroup {
				sources: vec![0, 1],
				coords: vec![tc(0, 1, 0)],
			},
			SignatureGroup {
				sources: vec![0, 1],
				coords: vec![tc(0, 2, 0)],
			},
		];

		let batches = partition_into_batches(groups, 4, 10);
		assert_eq!(batches.len(), 1);
		let total: usize = batches[0].iter().map(|g| g.coords.len()).sum();
		assert_eq!(total, 3);
	}

	#[test]
	fn test_partition_respects_batch_size() {
		let groups = vec![
			SignatureGroup {
				sources: vec![0],
				coords: (0..50).map(|i| tc(0, i, 0)).collect(),
			},
			SignatureGroup {
				sources: vec![1],
				coords: (0..50).map(|i| tc(1, i, 0)).collect(),
			},
			SignatureGroup {
				sources: vec![2],
				coords: (0..50).map(|i| tc(2, i, 0)).collect(),
			},
		];

		let batches = partition_into_batches(groups, 3, 40);
		assert!(batches.len() >= 4);

		for batch in &batches {
			let batch_tiles: usize = batch.iter().map(|g| g.coords.len()).sum();
			assert!(batch_tiles <= 40, "batch has {batch_tiles} tiles, exceeds limit of 40",);
		}

		let total: usize = batches.iter().flat_map(|b| b.iter()).map(|g| g.coords.len()).sum();
		assert_eq!(total, 150);
	}
}
