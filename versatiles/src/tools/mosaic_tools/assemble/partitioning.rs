//! PCA-based recursive bisection for batch grouping.
//!
//! Tiles are collapsed into signature groups (tiles sharing the same source set),
//! then recursively bisected using PCA to find the most divisive axis at each level.
//! This minimizes `Σ |unique_sources(batch)|` — the total number of source-opens
//! across all batches.

use std::collections::HashMap;
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
		return split_single_group(groups.into_iter().next().expect("groups.len() == 1"), batch_size);
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
///
/// All inner loops exploit sparsity: instead of iterating all `num_sources`
/// dimensions per group, we only touch the sources actually present in each
/// group and apply dense corrections once.  This makes the cost
/// `O(Σ|sources_i| + num_sources)` per iteration instead of
/// `O(groups × num_sources)`.
fn pca_bisect(groups: Vec<SignatureGroup>, num_sources: usize) -> (Vec<SignatureGroup>, Vec<SignatureGroup>) {
	let total_tiles: usize = groups.iter().map(|g| g.coords.len()).sum();

	// Compute weighted mean (sparse: only touch sources present in each group)
	let total_weight: f64 = groups.iter().map(|g| g.coords.len() as f64).sum();
	let mut mean = vec![0.0f64; num_sources];
	for g in &groups {
		let w = g.coords.len() as f64;
		for &s in &g.sources {
			mean[s] += w;
		}
	}
	for m in &mut mean {
		*m /= total_weight;
	}

	// Power iteration to find PC1
	let mut v = initial_vector(num_sources);

	for _ in 0..15 {
		power_iteration_step(&groups, &mean, &mut v, num_sources);
	}

	// Project each group onto PC1 and sort by score (sparse dot product)
	// score_i = Σ_{s ∈ sources_i} v[s] - mean_dot_v
	let mean_dot_v: f64 = mean.iter().zip(v.iter()).map(|(m, vi)| m * vi).sum();
	let mut scored: Vec<(f64, usize)> = groups
		.iter()
		.enumerate()
		.map(|(i, g)| {
			let sparse_sum: f64 = g.sources.iter().map(|&s| v[s]).sum();
			(sparse_sum - mean_dot_v, i)
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
		.map(|i| groups_by_idx[i].take().expect("each index visited exactly once"))
		.collect();
	let right: Vec<SignatureGroup> = indices_right
		.into_iter()
		.map(|i| groups_by_idx[i].take().expect("each index visited exactly once"))
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

/// One iteration of the power method, exploiting sparsity.
///
/// Decomposes `new_v[s] = Σ_i w_i * dot_i * (x_is - mean[s])` into:
/// - sparse part: for each group, add `w * dot` only to its source dimensions
/// - dense correction: subtract `mean[s] * Σ(w_i * dot_i)` once at the end
fn power_iteration_step(groups: &[SignatureGroup], mean: &[f64], v: &mut Vec<f64>, num_sources: usize) {
	// Precompute mean · v for sparse dot products
	let mean_dot_v: f64 = mean.iter().zip(v.iter()).map(|(m, vi)| m * vi).sum();

	let mut new_v = vec![0.0f64; num_sources];
	let mut sum_w_dot = 0.0f64;

	for g in groups {
		let w = g.coords.len() as f64;
		// dot = Σ_{s ∈ sources} v[s] - mean_dot_v
		let sparse_sum: f64 = g.sources.iter().map(|&s| v[s]).sum();
		let dot = sparse_sum - mean_dot_v;
		let w_dot = w * dot;
		sum_w_dot += w_dot;
		// Sparse accumulate: new_v[s] += w * dot for s ∈ sources
		for &s in &g.sources {
			new_v[s] += w_dot;
		}
	}

	// Dense correction: new_v[s] -= mean[s] * sum_w_dot
	for (s, nv) in new_v.iter_mut().enumerate() {
		*nv -= mean[s] * sum_w_dot;
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
#[allow(clippy::cast_possible_truncation)]
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
			assert!(batch_tiles <= 40, "batch has {batch_tiles} tiles, exceeds limit of 40");
		}

		let total: usize = batches.iter().flat_map(|b| b.iter()).map(|g| g.coords.len()).sum();
		assert_eq!(total, 150);
	}

	#[test]
	fn test_collapse_empty_map() {
		let map: HashMap<TileCoord, Vec<usize>> = HashMap::new();
		let groups = collapse_into_signature_groups(map);
		assert!(groups.is_empty());
	}

	#[test]
	fn test_collapse_single_tile() {
		let mut map = HashMap::new();
		map.insert(tc(0, 0, 0), vec![5, 3, 1]);
		let groups = collapse_into_signature_groups(map);
		assert_eq!(groups.len(), 1);
		assert_eq!(groups[0].sources, vec![1, 3, 5]); // sorted
		assert_eq!(groups[0].coords.len(), 1);
	}

	#[test]
	fn test_collapse_many_unique_signatures() {
		let mut map = HashMap::new();
		for i in 0..10u32 {
			map.insert(tc(0, i, 0), vec![i as usize]);
		}
		let groups = collapse_into_signature_groups(map);
		assert_eq!(groups.len(), 10);
	}

	#[test]
	fn test_collapse_all_same_signature() {
		let mut map = HashMap::new();
		for i in 0..5u32 {
			map.insert(tc(0, i, 0), vec![0, 1]);
		}
		let groups = collapse_into_signature_groups(map);
		assert_eq!(groups.len(), 1);
		assert_eq!(groups[0].coords.len(), 5);
	}

	#[test]
	fn test_partition_batch_size_one() {
		let groups = vec![
			SignatureGroup {
				sources: vec![0],
				coords: vec![tc(0, 0, 0), tc(0, 1, 0)],
			},
			SignatureGroup {
				sources: vec![1],
				coords: vec![tc(1, 0, 0)],
			},
		];

		let batches = partition_into_batches(groups, 2, 1);
		assert_eq!(batches.len(), 3); // each tile in its own batch

		for batch in &batches {
			let tiles: usize = batch.iter().map(|g| g.coords.len()).sum();
			assert_eq!(tiles, 1);
		}
	}

	#[test]
	fn test_partition_exact_fit() {
		// 6 tiles, batch_size=6 → should be 1 batch
		let groups = vec![
			SignatureGroup {
				sources: vec![0],
				coords: vec![tc(0, 0, 0), tc(0, 1, 0), tc(0, 2, 0)],
			},
			SignatureGroup {
				sources: vec![1],
				coords: vec![tc(1, 0, 0), tc(1, 1, 0), tc(1, 2, 0)],
			},
		];

		let batches = partition_into_batches(groups, 2, 6);
		assert_eq!(batches.len(), 1);
		let total: usize = batches[0].iter().map(|g| g.coords.len()).sum();
		assert_eq!(total, 6);
	}

	#[test]
	fn test_partition_single_tile_single_source() {
		let groups = vec![SignatureGroup {
			sources: vec![0],
			coords: vec![tc(0, 0, 0)],
		}];

		let batches = partition_into_batches(groups, 1, 100);
		assert_eq!(batches.len(), 1);
		assert_eq!(batches[0][0].coords.len(), 1);
	}

	#[test]
	fn test_partition_overlapping_sources_grouped() {
		// Groups with overlapping sources should tend to land in the same batch
		let groups = vec![
			SignatureGroup {
				sources: vec![0, 1, 2],
				coords: vec![tc(0, 0, 0), tc(0, 1, 0)],
			},
			SignatureGroup {
				sources: vec![0, 1],
				coords: vec![tc(0, 2, 0), tc(0, 3, 0)],
			},
			SignatureGroup {
				sources: vec![3, 4, 5],
				coords: vec![tc(1, 0, 0), tc(1, 1, 0)],
			},
			SignatureGroup {
				sources: vec![3, 4],
				coords: vec![tc(1, 2, 0), tc(1, 3, 0)],
			},
		];

		let batches = partition_into_batches(groups, 6, 4);
		assert_eq!(batches.len(), 2);

		// Each batch should have 4 tiles
		for batch in &batches {
			let tiles: usize = batch.iter().map(|g| g.coords.len()).sum();
			assert_eq!(tiles, 4);
		}

		// Check that overlapping sources end up together
		for batch in &batches {
			let all_sources: BTreeSet<usize> = batch.iter().flat_map(|g| g.sources.iter().copied()).collect();
			// Should be either {0,1,2} or {3,4,5} dominant
			assert!(
				all_sources.is_subset(&[0, 1, 2].into_iter().collect())
					|| all_sources.is_subset(&[3, 4, 5].into_iter().collect()),
				"batch has mixed source groups: {all_sources:?}"
			);
		}
	}

	#[test]
	fn test_partition_many_small_groups() {
		// 20 groups with 1 tile each, 4 distinct source sets
		let mut groups = Vec::new();
		for i in 0..20u32 {
			groups.push(SignatureGroup {
				sources: vec![(i % 4) as usize],
				coords: vec![tc(0, i, 0)],
			});
		}

		let batches = partition_into_batches(groups, 4, 5);
		let total: usize = batches.iter().flat_map(|b| b.iter()).map(|g| g.coords.len()).sum();
		assert_eq!(total, 20);

		for batch in &batches {
			let tiles: usize = batch.iter().map(|g| g.coords.len()).sum();
			assert!(tiles <= 5);
		}
	}

	// ─── initial_vector ───

	#[test]
	fn test_initial_vector_is_normalized() {
		let v = initial_vector(5);
		assert_eq!(v.len(), 5);
		let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
		assert!((norm - 1.0).abs() < 1e-10, "norm should be 1.0, got {norm}");
	}

	#[test]
	fn test_initial_vector_single_source() {
		let v = initial_vector(1);
		assert_eq!(v.len(), 1);
		assert!((v[0] - 1.0).abs() < 1e-10);
	}

	#[test]
	fn test_initial_vector_large() {
		let v = initial_vector(100);
		assert_eq!(v.len(), 100);
		let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
		assert!((norm - 1.0).abs() < 1e-10);
		// Values should be monotonically increasing (since input is [1,2,3,...])
		for i in 1..v.len() {
			assert!(v[i] > v[i - 1]);
		}
	}

	// ─── power_iteration_step ───

	#[test]
	fn test_power_iteration_produces_unit_vector() {
		let groups = vec![
			SignatureGroup {
				sources: vec![0, 1],
				coords: vec![tc(0, 0, 0)],
			},
			SignatureGroup {
				sources: vec![2, 3],
				coords: vec![tc(1, 0, 0)],
			},
		];
		let mean = vec![0.5, 0.5, 0.5, 0.5]; // uniform mean
		let mut v = initial_vector(4);

		power_iteration_step(&groups, &mean, &mut v, 4);

		let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
		assert!(
			(norm - 1.0).abs() < 1e-10,
			"output should be normalized, got norm={norm}"
		);
	}

	#[test]
	fn test_power_iteration_converges() {
		// Two clearly separable groups: {0,1} vs {2,3}
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
		let mean = vec![0.5, 0.5, 0.5, 0.5];
		let mut v = initial_vector(4);

		// Run several iterations
		for _ in 0..15 {
			power_iteration_step(&groups, &mean, &mut v, 4);
		}

		// The principal component should separate {0,1} from {2,3}
		// v[0] and v[1] should have the same sign, v[2] and v[3] the opposite
		assert!(
			(v[0].signum() == v[1].signum()) && (v[2].signum() == v[3].signum()),
			"PCA should group correlated sources: v={v:?}"
		);
		assert!(
			v[0].signum() != v[2].signum(),
			"PCA should separate disjoint groups: v={v:?}"
		);
	}

	#[test]
	fn test_power_iteration_weighted_by_tile_count() {
		// Group with more tiles should have more influence
		let groups = vec![
			SignatureGroup {
				sources: vec![0],
				coords: (0..100).map(|i| tc(0, i, 0)).collect(), // 100 tiles
			},
			SignatureGroup {
				sources: vec![1],
				coords: vec![tc(1, 0, 0)], // 1 tile
			},
		];
		let total_w: f64 = 101.0;
		let mean = vec![100.0 / total_w, 1.0 / total_w];
		let mut v = initial_vector(2);

		for _ in 0..15 {
			power_iteration_step(&groups, &mean, &mut v, 2);
		}

		// Both components should be non-zero (there's variance in both dimensions)
		assert!(v[0].abs() > 0.01, "v[0] too small: {v:?}");
		assert!(v[1].abs() > 0.01, "v[1] too small: {v:?}");
	}

	// ─── pca_bisect ───

	#[test]
	fn test_pca_bisect_two_disjoint_groups() {
		let groups = vec![
			SignatureGroup {
				sources: vec![0, 1],
				coords: vec![tc(0, 0, 0), tc(0, 1, 0)],
			},
			SignatureGroup {
				sources: vec![2, 3],
				coords: vec![tc(1, 0, 0), tc(1, 1, 0)],
			},
		];

		let (left, right) = pca_bisect(groups, 4);
		assert_eq!(left.len(), 1);
		assert_eq!(right.len(), 1);

		// Each side should have one group with 2 tiles
		assert_eq!(left[0].coords.len(), 2);
		assert_eq!(right[0].coords.len(), 2);

		// Sources should be separated
		let left_src: BTreeSet<usize> = left.iter().flat_map(|g| g.sources.iter().copied()).collect();
		let right_src: BTreeSet<usize> = right.iter().flat_map(|g| g.sources.iter().copied()).collect();
		assert!(left_src.is_disjoint(&right_src));
	}

	#[test]
	fn test_pca_bisect_balanced_split() {
		// 4 groups of 5 tiles each → bisect should give ~10 tiles per side
		let groups = vec![
			SignatureGroup {
				sources: vec![0],
				coords: (0..5).map(|i| tc(0, i, 0)).collect(),
			},
			SignatureGroup {
				sources: vec![1],
				coords: (0..5).map(|i| tc(1, i, 0)).collect(),
			},
			SignatureGroup {
				sources: vec![2],
				coords: (0..5).map(|i| tc(2, i, 0)).collect(),
			},
			SignatureGroup {
				sources: vec![3],
				coords: (0..5).map(|i| tc(3, i, 0)).collect(),
			},
		];

		let (left, right) = pca_bisect(groups, 4);
		let left_tiles: usize = left.iter().map(|g| g.coords.len()).sum();
		let right_tiles: usize = right.iter().map(|g| g.coords.len()).sum();
		assert_eq!(left_tiles + right_tiles, 20);

		// Both sides should have at least 1 group
		assert!(!left.is_empty());
		assert!(!right.is_empty());
	}

	#[test]
	fn test_pca_bisect_unequal_weights() {
		// One big group (90 tiles) and one small (10 tiles)
		let groups = vec![
			SignatureGroup {
				sources: vec![0],
				coords: (0..90).map(|i| tc(0, i, 0)).collect(),
			},
			SignatureGroup {
				sources: vec![1],
				coords: (0..10).map(|i| tc(1, i, 0)).collect(),
			},
		];

		let (left, right) = pca_bisect(groups, 2);
		let left_tiles: usize = left.iter().map(|g| g.coords.len()).sum();
		let right_tiles: usize = right.iter().map(|g| g.coords.len()).sum();
		assert_eq!(left_tiles + right_tiles, 100);
		// Both sides must have at least 1 group (clamped split)
		assert!(!left.is_empty());
		assert!(!right.is_empty());
	}

	// ─── split_single_group ───

	#[test]
	fn test_split_single_group_exact_division() {
		let g = SignatureGroup {
			sources: vec![0, 1],
			coords: (0..9).map(|i| tc(0, i, 0)).collect(),
		};
		let batches = split_single_group(g, 3);
		assert_eq!(batches.len(), 3);
		for batch in &batches {
			assert_eq!(batch.len(), 1); // one SignatureGroup per batch
			assert_eq!(batch[0].coords.len(), 3);
			assert_eq!(batch[0].sources, vec![0, 1]);
		}
	}

	#[test]
	fn test_split_single_group_with_remainder() {
		let g = SignatureGroup {
			sources: vec![5],
			coords: (0..7).map(|i| tc(0, i, 0)).collect(),
		};
		let batches = split_single_group(g, 3);
		assert_eq!(batches.len(), 3); // 3+3+1
		assert_eq!(batches[0][0].coords.len(), 3);
		assert_eq!(batches[1][0].coords.len(), 3);
		assert_eq!(batches[2][0].coords.len(), 1);
	}

	#[test]
	fn test_split_single_group_one_tile() {
		let g = SignatureGroup {
			sources: vec![0],
			coords: vec![tc(0, 0, 0)],
		};
		let batches = split_single_group(g, 5);
		assert_eq!(batches.len(), 1);
		assert_eq!(batches[0][0].coords.len(), 1);
	}

	#[test]
	fn test_split_single_group_sorts_spatially() {
		let g = SignatureGroup {
			sources: vec![0],
			// Coords in reverse order
			coords: vec![tc(0, 3, 0), tc(0, 1, 0), tc(0, 2, 0), tc(0, 0, 0)],
		};
		let batches = split_single_group(g, 2);
		assert_eq!(batches.len(), 2);
		// First batch should have the lowest x values
		let first_xs: Vec<u32> = batches[0][0].coords.iter().map(|c| c.x).collect();
		let second_xs: Vec<u32> = batches[1][0].coords.iter().map(|c| c.x).collect();
		assert_eq!(first_xs, vec![0, 1]);
		assert_eq!(second_xs, vec![2, 3]);
	}

	// ─── partition edge cases via integration ───

	#[test]
	fn test_partition_deep_recursion() {
		// Many groups that require multiple levels of PCA bisection
		let groups: Vec<SignatureGroup> = (0..16)
			.map(|i| SignatureGroup {
				sources: vec![i],
				coords: (0..10).map(|j| tc(i as u8, j, 0)).collect(),
			})
			.collect();

		let batches = partition_into_batches(groups, 16, 20);
		let total: usize = batches.iter().flat_map(|b| b.iter()).map(|g| g.coords.len()).sum();
		assert_eq!(total, 160);

		for batch in &batches {
			let tiles: usize = batch.iter().map(|g| g.coords.len()).sum();
			assert!(tiles <= 20);
		}
	}

	#[test]
	fn test_partition_two_groups_one_tile_each() {
		let groups = vec![
			SignatureGroup {
				sources: vec![0],
				coords: vec![tc(0, 0, 0)],
			},
			SignatureGroup {
				sources: vec![1],
				coords: vec![tc(1, 0, 0)],
			},
		];

		// batch_size=1 forces split
		let batches = partition_into_batches(groups, 2, 1);
		assert_eq!(batches.len(), 2);
	}
}
