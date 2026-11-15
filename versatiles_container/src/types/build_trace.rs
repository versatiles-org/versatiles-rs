//! Which operation of a built pipeline makes its pyramid reach how deep.

use std::{
	fmt::Write as _,
	sync::{Arc, RwLock},
};

use versatiles_core::TilePyramid;

use crate::TileSourceMetadata;

/// Labels longer than this are cut, so that a refusal stays readable.
const MAX_LABEL_LEN: usize = 100;

/// One operation of a built pipeline, with the operations it reads from.
///
/// A pipeline records one per operation while it is built. Each one watches
/// the pyramid of its operation, so a pyramid computed later, the first time
/// something asks for it, is seen too. A conversion that is refused for its
/// size uses the trace to name the operations that make it reach so deep:
/// most often a source such as `from_color`, which has a tile at every level
/// up to 30, while the rest of the pipeline ends far lower.
#[derive(Debug)]
pub struct BuildTrace {
	label: String,
	pyramid: Arc<RwLock<Option<Arc<TilePyramid>>>>,
	inputs: Vec<Arc<BuildTrace>>,
}

impl BuildTrace {
	/// Records the operation `label` that produces `metadata`, reading from
	/// `inputs`. A label longer than 100 characters is cut.
	#[must_use]
	pub fn new(label: &str, metadata: &TileSourceMetadata, inputs: Vec<Arc<BuildTrace>>) -> Arc<Self> {
		let mut label: String = label.chars().take(MAX_LABEL_LEN.saturating_add(1)).collect();
		if label.chars().count() > MAX_LABEL_LEN {
			label.pop();
			label.push('…');
		}
		Arc::new(Self {
			label,
			pyramid: metadata.pyramid_cell(),
			inputs,
		})
	}

	/// Whether this trace was recorded for the source that produces
	/// `metadata`, and not for another pipeline built under the same runtime.
	#[must_use]
	pub fn describes(&self, metadata: &TileSourceMetadata) -> bool {
		Arc::ptr_eq(&self.pyramid, &metadata.pyramid_cell())
	}

	/// The operation's top level, if its pyramid is known by now.
	fn level_max(&self) -> Option<u8> {
		self.pyramid.read().expect("poisoned RwLock").as_ref()?.level_max()
	}

	/// The operations that make the pipeline reach `level`. Each one reaches
	/// it, and none of its inputs does: it adds the levels itself.
	fn reaching<'a>(&'a self, level: u8, found: &mut Vec<&'a BuildTrace>) {
		if self.level_max().is_none_or(|max| max < level) {
			return;
		}
		let before = found.len();
		for input in &self.inputs {
			input.reaching(level, found);
		}
		if found.len() == before {
			found.push(self);
		}
	}

	/// The top level the pipeline has without the operations that reach
	/// `level`, if any part of it ends below.
	fn level_without(&self, level: u8) -> Option<u8> {
		match self.level_max()? {
			max if max < level => Some(max),
			_ => self.inputs.iter().filter_map(|input| input.level_without(level)).max(),
		}
	}

	/// A sentence naming the operations that make the pipeline reach `level`,
	/// and the level where the rest of it ends.
	///
	/// `None` if there is nothing to add: the pipeline is a single operation,
	/// or its pyramid is not known.
	#[must_use]
	pub fn explain_level(&self, level: u8) -> Option<String> {
		let mut found = Vec::new();
		self.reaching(level, &mut found);
		let rest = self.level_without(level);
		if found.is_empty() || (rest.is_none() && found.len() == 1 && std::ptr::eq(found[0], self)) {
			return None;
		}

		let mut names = String::new();
		for (index, trace) in found.iter().enumerate() {
			if index > 0 {
				names.push_str(if index + 1 == found.len() { " and " } else { ", " });
			}
			write!(names, "`{}`", trace.label).expect("writing to a String");
		}
		Some(match rest {
			Some(rest) => {
				let verb = if found.len() == 1 { "goes" } else { "go" };
				format!(
					"Only {names} {verb} beyond level {rest}, where the rest of the pipeline ends: convert with --max-zoom {rest}, or put `filter level_max={rest}` after {}.",
					if found.len() == 1 { "it" } else { "each of them" }
				)
			}
			None => format!("The levels come from {names}."),
		})
	}
}

#[cfg(test)]
mod tests {
	use versatiles_core::{TileCompression, TileFormat};

	use super::*;
	use crate::Traversal;

	fn metadata(level_max: Option<u8>) -> TileSourceMetadata {
		TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::Uncompressed,
			Traversal::ANY,
			level_max.map(TilePyramid::new_full_up_to),
		)
	}

	fn node(label: &str, level_max: Option<u8>, inputs: Vec<Arc<BuildTrace>>) -> Arc<BuildTrace> {
		BuildTrace::new(label, &metadata(level_max), inputs)
	}

	/// The case of issue #330: a colour beneath data that ends at 12.
	#[test]
	fn names_the_source_that_goes_deeper_than_the_rest() {
		let data = node(
			"filter level_max=12",
			Some(12),
			vec![node("from_color color=f00", Some(30), vec![])],
		);
		let background = node("from_color color=242d41", Some(30), vec![]);
		let stack = node("from_stacked_raster", Some(30), vec![data, background]);

		assert_eq!(
			stack.explain_level(30).unwrap(),
			"Only `from_color color=242d41` goes beyond level 12, where the rest of the pipeline ends: convert with --max-zoom 12, or put `filter level_max=12` after it."
		);
	}

	/// A transform that adds levels is named, not the source beneath it.
	#[test]
	fn names_a_transform_that_adds_levels() {
		let overscale = node(
			"raster_overscale level_max=30",
			Some(30),
			vec![node("from_container filename=a.versatiles", Some(14), vec![])],
		);
		let message = overscale.explain_level(30).unwrap();
		assert!(
			message.starts_with("Only `raster_overscale level_max=30` goes beyond level 14"),
			"{message}"
		);
	}

	#[test]
	fn names_every_source_that_goes_that_deep() {
		let stack = node(
			"from_stacked_raster",
			Some(30),
			vec![
				node("from_color", Some(30), vec![]),
				node("from_container", Some(10), vec![]),
				node("from_tile", Some(30), vec![]),
			],
		);
		let message = stack.explain_level(20).unwrap();
		assert!(
			message.starts_with("Only `from_color` and `from_tile` go beyond level 10"),
			"{message}"
		);
		assert!(message.contains("after each of them"), "{message}");
	}

	/// Passing a source's levels through does not make a transform the cause.
	#[test]
	fn a_transform_that_keeps_the_levels_is_not_named() {
		let chain = node("raster_levels", Some(30), vec![node("from_color", Some(30), vec![])]);
		assert_eq!(chain.explain_level(30).unwrap(), "The levels come from `from_color`.");
	}

	#[test]
	fn a_single_operation_needs_no_explanation() {
		assert_eq!(node("from_color", Some(30), vec![]).explain_level(30), None);
	}

	#[test]
	fn an_unknown_pyramid_explains_nothing() {
		let stack = node("from_stacked_raster", None, vec![node("from_color", Some(30), vec![])]);
		assert_eq!(stack.explain_level(30), None);
	}

	/// The trace watches the pyramid, so one computed after the build counts.
	#[test]
	fn a_pyramid_computed_later_is_seen() {
		let lazy = metadata(None);
		let trace = BuildTrace::new("from_container", &lazy, vec![]);
		let stack = node(
			"from_stacked_raster",
			Some(30),
			vec![trace, node("from_color", Some(30), vec![])],
		);
		assert!(stack.explain_level(30).unwrap().starts_with("The levels come from"));

		lazy
			.get_or_compute_tile_pyramid(|| Ok(TilePyramid::new_full_up_to(12)))
			.unwrap();
		assert!(
			stack
				.explain_level(30)
				.unwrap()
				.starts_with("Only `from_color` goes beyond level 12")
		);
	}

	#[test]
	fn a_trace_describes_only_its_own_source() {
		let own = metadata(Some(3));
		let trace = BuildTrace::new("from_color", &own, vec![]);
		assert!(trace.describes(&own));
		assert!(!trace.describes(&own.clone()));
		assert!(!trace.describes(&metadata(Some(3))));
	}

	#[test]
	fn a_long_label_is_cut() {
		let trace = node(&"x".repeat(150), Some(1), vec![]);
		assert_eq!(trace.label.chars().count(), MAX_LABEL_LEN + 1);
		assert!(trace.label.ends_with('…'));
	}
}
