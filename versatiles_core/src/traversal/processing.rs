use crate::{TileBBox, TileBBoxPyramid, Traversal, TraversalOrder};
use anyhow::{Result, anyhow, bail, ensure};
use std::{
	collections::{HashMap, HashSet},
	vec,
};
use versatiles_derive::context;

#[derive(Debug, Clone)]
pub enum TraversalTranslationStep {
	Push(Vec<TileBBox>, usize),
	Pop(usize, TileBBox),
	Stream(Vec<TileBBox>, TileBBox),
}

#[context("Could not find a way to translate traversals from {traversal_read:?} to {traversal_write:?}")]
pub fn translate_traversals(
	pyramid: &TileBBoxPyramid,
	traversal_read: &Traversal,
	traversal_write: &Traversal,
) -> Result<Vec<TraversalTranslationStep>> {
	if let Ok(traversal) = traversal_read.get_intersected(traversal_write) {
		return Ok(traversal
			.traverse_pyramid(pyramid)?
			.into_iter()
			.map(|b| TraversalTranslationStep::Stream(vec![b], b))
			.collect::<Vec<_>>());
	}

	if traversal_write.order() == &TraversalOrder::AnyOrder {
		#[allow(clippy::collapsible_if)]
		if traversal_read.size.max_size()? <= traversal_write.size.min_size()? {
			let write_size = traversal_write.size.min_size()?;
			let read_bboxes = traversal_read.traverse_pyramid(pyramid)?;

			use TraversalTranslationStep::*;

			let mut map_write = HashMap::<TileBBox, (usize, TileBBox)>::new();
			let mut steps: Vec<TraversalTranslationStep> = vec![];
			for bbox_read in read_bboxes {
				let bbox_write = bbox_read.rounded(write_size);
				let n = map_write.len();
				let index = map_write.entry(bbox_write).or_insert((n, bbox_write)).0;
				steps.push(Push(vec![bbox_read], index));
			}

			for (index, bbox_write) in map_write.into_values() {
				steps.push(Pop(index, bbox_write));
			}

			simplify_steps(&mut steps)?;
			verify_steps(
				&steps,
				*traversal_read.order(),
				traversal_read.size.max_size()?,
				*traversal_write.order(),
				write_size,
				pyramid,
			)?;

			return Ok(steps);
		}
	}

	bail!("Could not find a way to translate traversals.")
}

fn simplify_steps(steps: &mut Vec<TraversalTranslationStep>) -> Result<()> {
	use TraversalTranslationStep::*;

	// ----- Step 1: Merge neighbouring Pushes with the same index -----
	*steps = {
		let mut result: Vec<TraversalTranslationStep> = Vec::with_capacity(steps.len());
		for step in steps.drain(..) {
			match (&mut result.last_mut(), &step) {
				(Some(Push(prev_v, prev_idx)), Push(v, idx)) if prev_idx == idx => {
					prev_v.extend(v.iter().copied());
				}
				_ => result.push(step),
			}
		}
		result
	};

	// ----- Step 2: Move each Pop right after the last Push with the same index -----
	*steps = {
		let mut result: Vec<TraversalTranslationStep> = Vec::with_capacity(steps.len());

		for step in steps.drain(..) {
			match step {
				Push(bboxes, idx) => {
					result.push(Push(bboxes, idx));
				}
				Pop(idx, bbox) => {
					let pos = result
						.iter()
						.rposition(|s| matches!(s, Push(_, i) if *i == idx))
						.ok_or(anyhow!("Could not find Push for index {idx}"))?;
					result.insert(pos + 1, Pop(idx, bbox));
				}
				Stream(bboxes, bbox) => {
					result.push(Stream(bboxes, bbox));
				}
			}
		}
		result
	};

	// ----- Step 3: Replace a single neighbouring Push + Pop (same index) with Stream -----
	*steps = {
		let mut result: Vec<TraversalTranslationStep> = Vec::with_capacity(steps.len());
		let mut count: HashMap<usize, u32> = HashMap::new();
		for step in steps.drain(..) {
			match step {
				Push(bboxes, idx) => {
					*count.entry(idx).or_insert(0) += 1;
					result.push(Push(bboxes, idx));
				}
				Pop(idx, bbox) => {
					let last_entry_option: Option<TraversalTranslationStep> = result.last().cloned();
					if let Some(last_entry) = last_entry_option
						&& let Push(bboxes, last_idx) = last_entry
						&& last_idx == idx
						&& count.get(&idx) == Some(&1)
					{
						result.pop();
						result.push(Stream(bboxes.clone(), bbox));
						continue;
					}
					result.push(Pop(idx, bbox));
				}
				Stream(bboxes, bbox) => {
					result.push(Stream(bboxes, bbox));
				}
			}
		}
		result
	};

	// ---- Step 4: Renumber indices ----
	let mut index_map: HashMap<usize, usize> = HashMap::new();
	for step in steps.iter_mut() {
		match step {
			Push(_, old_idx) | Pop(old_idx, _) => {
				let n = index_map.len();
				let new_idx = index_map.entry(*old_idx).or_insert(n);
				*old_idx = *new_idx;
			}
			Stream(_, _) => {}
		}
	}

	Ok(())
}

fn verify_steps(
	steps: &[TraversalTranslationStep],
	read_order: TraversalOrder,
	read_size: u32,
	write_order: TraversalOrder,
	write_size: u32,
	pyramid: &TileBBoxPyramid,
) -> Result<()> {
	use TraversalTranslationStep::*;

	// Check order of Pushes and Pops
	{
		let mut pushes = HashMap::<usize, Vec<TileBBox>>::new();
		let mut pops = HashSet::<usize>::new();
		for step in steps {
			match step {
				Push(bboxes, idx) => {
					ensure!(!pops.contains(idx), "Push follows Pop {idx}");
					pushes.entry(*idx).or_default().extend(bboxes);
				}
				Pop(idx, bbox) => {
					ensure!(pushes.contains_key(idx), "Pop without Push {idx}");
					ensure!(!pops.contains(idx), "Double Pop {idx}");
					for push_bbox in pushes.get(idx).unwrap() {
						ensure!(!push_bbox.is_empty(), "Pushed BBox {push_bbox:?} is empty");
						ensure!(
							bbox.try_contains_bbox(push_bbox)?,
							"Pushed BBox {push_bbox:?} not contained in Pop {bbox:?}"
						);
					}
					pops.insert(*idx);
				}
				_ => {}
			}
		}
		for idx in pushes.keys() {
			ensure!(pops.contains(idx), "Push without Pop {idx}");
		}
	}

	fn check_order(step_bboxes: &[TileBBox], order: TraversalOrder, size: u32, pyramid: &TileBBoxPyramid) -> Result<()> {
		let mut lookup = HashMap::<(u8, u32, u32), bool>::new();

		// verify sizes
		for bbox in step_bboxes {
			ensure!(bbox.width() <= size);
			ensure!(bbox.height() <= size);

			let scaled = bbox.scaled_down(size);
			ensure!(scaled.width() == 1);
			ensure!(scaled.height() == 1);

			let key = (scaled.level, scaled.x_min(), scaled.y_min());
			ensure!(!lookup.contains_key(&key), "Duplicate read of bbox {bbox:?}");
			lookup.insert(key, false);
		}

		// verify order
		if !order.verify_order(step_bboxes, size) {
			bail!("Steps are not in {order:?} order");
		}

		let read_bboxes = Traversal::new(order, size, size)?.traverse_pyramid(pyramid)?;
		for bbox in &read_bboxes {
			let scaled = bbox.scaled_down(size);
			ensure!(scaled.width() == 1);
			ensure!(scaled.height() == 1);

			let key = (scaled.level, scaled.x_min(), scaled.y_min());
			ensure!(lookup.contains_key(&key), "Missing read of bbox {bbox:?}");
			ensure!(!lookup.get(&key).unwrap(), "Duplicate (2) read of bbox {bbox:?}");
			lookup.insert(key, true);
		}

		for (key, value) in lookup {
			ensure!(value, "Missing (2) read of bbox key {key:?}");
		}

		Ok(())
	}

	// Check Read operations
	{
		// get all read bboxes
		let step_bboxes = steps
			.iter()
			.flat_map(|s| match s {
				Push(bboxes, _) | Stream(bboxes, _) => bboxes.clone(),
				Pop(_, _) => vec![],
			})
			.collect::<Vec<_>>();
		check_order(&step_bboxes, read_order, read_size, pyramid)?;
	}

	// Check Write Operations
	{
		// get all written bboxes
		let step_bboxes = steps
			.iter()
			.filter_map(|s| match s {
				Pop(_, bbox) | Stream(_, bbox) => Some(*bbox),
				Push(_, _) => None,
			})
			.collect::<Vec<_>>();

		check_order(&step_bboxes, write_order, write_size, pyramid)?;
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::GeoBBox;
	use TraversalOrder::*;

	fn test(
		order_read: TraversalOrder,
		size_read_min: u32,
		size_read_max: u32,
		order_write: TraversalOrder,
		size_write_min: u32,
		size_write_max: u32,
	) -> Vec<String> {
		let pyramid = TileBBoxPyramid::from_geo_bbox(13, 15, &GeoBBox::new(12.0, 13.0, 14.0, 15.0).unwrap());
		let read = Traversal::new(order_read, size_read_min, size_read_max).unwrap();
		let write = Traversal::new(order_write, size_write_min, size_write_max).unwrap();
		let steps = translate_traversals(&pyramid, &read, &write).unwrap();
		steps
			.iter()
			.map(|s| {
				use TraversalTranslationStep::*;
				fn f(bbox: &TileBBox) -> String {
					format!(
						"[{}: {},{} {}x{}]",
						bbox.level,
						bbox.x_min(),
						bbox.y_min(),
						bbox.width(),
						bbox.height()
					)
				}
				fn fs(bbox: &[TileBBox]) -> String {
					bbox.iter().map(f).collect::<Vec<_>>().join(", ")
				}
				match s {
					Push(bboxes, i) => {
						format!("Push: {}; {i}", fs(bboxes))
					}
					Pop(i, bbox) => {
						format!("Pop: {i}; {}", f(bbox))
					}
					Stream(bboxes, bbox) => {
						if bboxes.len() == 1 && bboxes[0] == *bbox {
							format!("Stream: {}", f(bbox))
						} else {
							format!("Stream: {}; {}", fs(bboxes), f(bbox))
						}
					}
				}
			})
			.collect()
	}

	#[test]
	fn translate_any2any() {
		assert_eq!(
			test(AnyOrder, 1, 256, AnyOrder, 1, 256),
			&[
				"Stream: [13: 4369,3750 46x48]",
				"Stream: [14: 8738,7501 92x95]",
				"Stream: [15: 17476,15002 183x102]",
				"Stream: [15: 17476,15104 183x87]"
			]
		);
	}

	#[test]
	fn translate_depthfirst2any() {
		assert_eq!(
			test(DepthFirst, 1, 256, AnyOrder, 1, 256),
			&[
				"Stream: [15: 17476,15002 183x102]",
				"Stream: [15: 17476,15104 183x87]",
				"Stream: [14: 8738,7501 92x95]",
				"Stream: [13: 4369,3750 46x48]"
			]
		);
	}

	#[test]
	fn translate_depthfirst2any_smaller() {
		assert_eq!(
			test(DepthFirst, 1, 128, AnyOrder, 256, 256),
			&[
				"Stream: [15: 17476,15002 60x102], [15: 17536,15002 123x102]; [15: 17408,14848 256x256]",
				"Push: [14: 8738,7501 92x51]; 0",
				"Stream: [15: 17476,15104 60x87], [15: 17536,15104 123x87]; [15: 17408,15104 256x256]",
				"Push: [14: 8738,7552 92x44]; 0",
				"Pop: 0; [14: 8704,7424 256x256]",
				"Stream: [13: 4369,3750 46x48]; [13: 4352,3584 256x256]"
			]
		);
	}
}
