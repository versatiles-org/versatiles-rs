use std::{collections::HashMap, vec};

use crate::{TileBBox, TileBBoxPyramid, Traversal};
use anyhow::{Result, anyhow, bail};
use versatiles_derive::context;

#[derive(Debug, Clone)]
pub enum TraversalTranslationStep {
	Push(TileBBox, usize),
	Pop(usize, TileBBox),
	Stream(TileBBox),
}

#[context("Could not find a way to translate traversals from {traversal_read:?} to {traversal_write:?}")]
pub fn translate_traversals<'a>(
	pyramid: &TileBBoxPyramid,
	traversal_read: &Traversal,
	traversal_write: &Traversal,
) -> Result<Vec<TraversalTranslationStep>> {
	if let Ok(traversal) = traversal_read.get_intersected(traversal_write) {
		return Ok(traversal
			.traverse_pyramid(pyramid)?
			.into_iter()
			.map(|bbox| TraversalTranslationStep::Stream(bbox))
			.collect::<Vec<_>>());
	};

	if let Ok(order) = traversal_read.order.get_intersected(&traversal_write.order) {
		if traversal_read.size.max_size()? < traversal_write.size.min_size()? {
			let read_size = traversal_read.size.max_size()?;
			let write_size = traversal_write.size.min_size()?;
			let read_bboxes = Traversal::new(order, read_size, read_size)?.traverse_pyramid(pyramid)?;
			let write_bboxes = Traversal::new(order, write_size, write_size)?.traverse_pyramid(pyramid)?;

			use TraversalTranslationStep::*;

			let mut map = HashMap::<TileBBox, usize>::new();

			let mut steps: Vec<TraversalTranslationStep> = vec![];
			for bbox_read in read_bboxes {
				let bbox_map = bbox_read.get_rounded(write_size);
				let n = map.len();
				let index = map.entry(bbox_map).or_insert(n);

				steps.push(Push(bbox_read, *index));
			}

			for bbox_write in write_bboxes {
				let bbox_map = bbox_write.get_rounded(write_size);
				let index = map
					.get(&bbox_map)
					.ok_or_else(|| anyhow!("Could not find mapping for {bbox_map:?}"))?;

				let last_pos = steps
					.iter()
					.rposition(|step| {
						if let TraversalTranslationStep::Push(_, i) = step {
							*i == *index
						} else {
							false
						}
					})
					.unwrap();

				steps.insert(last_pos + 1, Pop(*index, bbox_write));
			}

			return Ok(steps);
		}
	}

	bail!("Could not find a way to translate traversals.")
}
