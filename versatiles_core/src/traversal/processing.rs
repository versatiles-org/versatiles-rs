use std::{collections::HashMap, vec};

use crate::{TileBBox, TileBBoxPyramid, Traversal, TraversalOrder};
use anyhow::{Result, bail};
use versatiles_derive::context;

#[derive(Debug, Clone)]
pub enum TraversalTranslationStep {
	Push(Vec<TileBBox>, usize),
	Pop(usize, TileBBox),
	Stream(TileBBox),
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
			.map(TraversalTranslationStep::Stream)
			.collect::<Vec<_>>());
	};

	if traversal_write.order() == &TraversalOrder::AnyOrder {
		#[allow(clippy::collapsible_if)]
		if traversal_read.size.max_size()? <= traversal_write.size.min_size()? {
			let write_size = traversal_write.size.min_size()?;
			let read_bboxes = traversal_read.traverse_pyramid(pyramid)?;

			use TraversalTranslationStep::*;

			let mut map_write = HashMap::<TileBBox, (usize, TileBBox)>::new();
			let mut steps: Vec<TraversalTranslationStep> = vec![];
			for bbox_read in read_bboxes {
				let bbox_write = bbox_read.get_rounded(write_size);
				let n = map_write.len();
				let index = map_write.entry(bbox_write).or_insert((n, bbox_write)).0;
				if let Some(Push(bboxes, i)) = steps.last_mut() {
					if *i == index {
						bboxes.push(bbox_read);
						continue;
					}
				}
				steps.push(Push(vec![bbox_read], index));
			}

			for (index, bbox_write) in map_write.into_values() {
				let last_pos = steps
					.iter()
					.rposition(|step| {
						if let TraversalTranslationStep::Push(_, i) = step {
							*i == index
						} else {
							false
						}
					})
					.unwrap();

				steps.insert(last_pos + 1, Pop(index, bbox_write));
			}

			return Ok(steps);
		}
	}

	bail!("Could not find a way to translate traversals.")
}
