use std::{collections::HashSet, sync::Arc};

use anyhow::{Context, Result, anyhow, ensure};
use async_trait::async_trait;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileCoord, TileJSON, TilePyramid, TileStream};
use versatiles_derive::context;
use versatiles_geometry::{
	geo::GeoProperties,
	vector_tile::{VectorTile, VectorTileLayer},
};

use crate::{
	PipelineFactory, build_error,
	helpers::feature_expr::{CelExpression, ExprScope, FeatureExpr},
	operations::transform::{DynTileTransform, TileTransform},
	vpl::{VPLNode, VPLPipeline},
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Runs part of each vector tile through other operations, chosen by an expression.
///
/// The features for which `expr` is true go through the first source, the
/// `then` branch. The others go through the second, the `else` branch, or pass
/// through unchanged when there is none. Both halves are then put back
/// together into one tile, with the features of each layer in their original
/// order.
///
/// ```vpl
/// from_container filename="osm.versatiles"
///   | vector_branch expr="$layer == 'place_labels'" [
///       vector_mask geojson="berlin.geojson"
///     ]
/// ```
///
/// That cuts the place labels to Berlin and leaves every other layer as it was.
///
/// `expr` is written as in `vector_filter_features`, with `$zoom`, `$layer`
/// and `$props` beside the feature's own properties. It is answered for as much
/// of a tile at once as it can be: an expression reading only `$zoom` sends
/// each tile to one branch without decoding it, and one reading `$layer` but no
/// property sends whole layers.
///
/// A branch begins with a transform rather than a read, because it starts from
/// the tiles arriving here. It may only contain operations that make each tile
/// from that tile alone, such as filtering, masking or changing properties. An
/// operation that reads other tiles, such as `vector_overzoom`, is refused; put
/// it before or after the branch.
///
/// Branches nest, which is how to write an "else if". This keeps motorways in
/// Germany, other streets in Berlin, and everything else everywhere:
///
/// ```vpl
/// from_container filename="osm.versatiles"
///   | vector_branch expr="$layer == 'streets' && kind == 'motorway'" [
///       vector_mask geojson="germany.geojson",
///       vector_branch expr="$layer == 'streets'" [
///         vector_mask geojson="berlin.geojson"
///       ]
///     ]
/// ```
struct Args {
	/// Boolean expression choosing the features for the first branch.
	expr: CelExpression,

	/// The branch for the features `expr` chooses, then optionally the branch
	/// for the others. Each begins with a transform and starts from the tiles
	/// arriving at this operation.
	#[vpl(min_sources = 1, max_sources = 2, branches)]
	sources: Vec<VPLPipeline>,
}

/// The transforms of one branch, in order. Empty for a missing `else`, which
/// leaves its features as they are.
#[derive(Debug, Default)]
struct Branch {
	steps: Vec<Arc<dyn DynTileTransform>>,
}

impl Branch {
	/// Runs `tile` through every step, stopping when one drops it.
	fn run(&self, coord: &TileCoord, mut tile: Tile) -> Result<Option<Tile>> {
		for step in &self.steps {
			match step
				.apply(coord, tile)
				.with_context(|| format!("in `{}` inside `vector_branch`", step.tag()))?
			{
				Some(next) => tile = next,
				None => return Ok(None),
			}
		}
		Ok(Some(tile))
	}
}

/// A branch together with what its tiles look like when they come out of it.
struct Described {
	branch: Branch,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
}

impl Described {
	/// A branch that changes nothing.
	fn unchanged(source: &dyn TileSource) -> Self {
		Described {
			branch: Branch::default(),
			metadata: source.metadata().clone(),
			tilejson: source.tilejson().clone(),
		}
	}

	/// Builds the branch at `index` of the node from `pipeline`, one transform
	/// at a time, each described with what the steps before it made.
	async fn build(
		pipeline: VPLPipeline,
		index: usize,
		source: &dyn TileSource,
		factory: &PipelineFactory,
	) -> Result<Self> {
		let mut described = Self::unchanged(source);
		for (step_index, node) in pipeline.pipeline.into_iter().enumerate() {
			let name = node.name.clone();
			let input = BranchInput {
				source,
				metadata: described.metadata.clone(),
				tilejson: described.tilejson.clone(),
			};
			let step = Self::step(node, &input, factory)
				.await
				.map_err(|err| build_error::in_source(build_error::at_node(err, step_index, &name), index))?;

			// What `TransformOp::new` does to describe the tiles a transform makes.
			step.adjust_metadata(&mut described.metadata);
			step.adjust_tilejson(&mut described.tilejson);
			described.metadata.update_tilejson(&mut described.tilejson);
			described.branch.steps.push(step);
		}
		Ok(described)
	}

	async fn step(
		node: VPLNode,
		input: &BranchInput<'_>,
		factory: &PipelineFactory,
	) -> Result<Arc<dyn DynTileTransform>> {
		let name = node.name.clone();
		ensure!(
			!factory.has_read_operation(&name),
			"'{name}' is a read operation, but a branch starts from the tiles arriving at `vector_branch`, so it begins with a transform"
		);
		factory.tile_transform_from_node(node, input).await?.ok_or_else(|| {
			anyhow!(
				"'{name}' cannot run inside `vector_branch`: it reads tiles other than the one it makes. \
				 Put it before or after the branch."
			)
		})
	}
}

/// What a step of a branch is built for: the tiles arriving at the branch, as
/// the steps before it changed them.
///
/// Building a step only asks about its input — metadata, `TileJSON`, pyramid —
/// so this answers from what the earlier steps reported and passes the rest to
/// the source the branch starts from.
#[derive(Debug)]
struct BranchInput<'a> {
	source: &'a dyn TileSource,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
}

#[async_trait]
impl TileSource for BranchInput<'_> {
	fn source_type(&self) -> Arc<SourceType> {
		self.source.source_type()
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		match self.metadata.tile_pyramid() {
			Some(pyramid) => Ok(pyramid),
			None => self.source.tile_pyramid().await,
		}
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		self.source.tile_stream(bbox).await
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.source.tile_coord_stream(bbox).await
	}
}

#[derive(Debug)]
pub(crate) struct Operation {
	expr: FeatureExpr,
	then: Branch,
	otherwise: Branch,
	/// The union of what the branches can produce, when a branch changed the
	/// pyramid; `None` when neither did.
	pyramid: Option<TilePyramid>,
	/// Both branches' `TileJSON`, merged.
	tilejson: TileJSON,
}

impl Operation {
	#[context("Building vector_branch operation in VPL node {:?}", vpl_node.name)]
	async fn build_transform(
		vpl_node: VPLNode,
		source: &dyn TileSource,
		factory: &PipelineFactory,
	) -> Result<Operation> {
		let args = Args::from_vpl_node(&vpl_node)?;
		ensure!(
			(1..=2).contains(&args.sources.len()),
			"vector_branch takes a `then` branch and optionally an `else` branch, not {}",
			args.sources.len()
		);
		let expr = FeatureExpr::new(args.expr.as_str())?;

		let mut pipelines = args.sources.into_iter();
		let then_pipeline = pipelines.next().expect("one source at least, ensured above");
		let then = Described::build(then_pipeline, 0, source, factory).await?;
		let otherwise = match pipelines.next() {
			Some(pipeline) => Described::build(pipeline, 1, source, factory).await?,
			None => Described::unchanged(source),
		};

		let format = *source.metadata().tile_format();
		for (described, name) in [(&then, "then"), (&otherwise, "else")] {
			let made = *described.metadata.tile_format();
			ensure!(
				made == format,
				"the {name} branch turns {format} tiles into {made}, but both halves of a tile have to come back as {format} to be put together again"
			);
		}

		let input_pyramid = source.metadata().tile_pyramid();
		let pyramid =
			if then.metadata.tile_pyramid() == input_pyramid && otherwise.metadata.tile_pyramid() == input_pyramid {
				None
			} else {
				let mut union = TilePyramid::new_empty();
				for described in [&then, &otherwise] {
					let pyramid = match described.metadata.tile_pyramid() {
						Some(pyramid) => pyramid,
						None => source.tile_pyramid().await?,
					};
					union.union(&pyramid);
				}
				Some(union)
			};

		let mut tilejson = then.tilejson.clone();
		tilejson.merge(&otherwise.tilejson)?;

		Ok(Operation {
			expr,
			then: then.branch,
			otherwise: otherwise.branch,
			pyramid,
			tilejson,
		})
	}

	/// Splits a decoded tile into the part `expr` chooses and the rest.
	///
	/// Also returns the layers split feature by feature, whose parts have to be
	/// put back in order, and of those the ones numbered here. A layer already
	/// numbered belongs to an enclosing `vector_branch`, whose numbers are kept so
	/// that it can restore the order of the whole layer.
	fn split(&self, coord: &TileCoord, tile: VectorTile) -> Result<(VectorTile, VectorTile, Split)> {
		let by_layer = self.expr.scope() == ExprScope::Layer;
		let mut chosen_layers = Vec::new();
		let mut other_layers = Vec::new();
		let mut split = Split::default();

		for mut layer in tile.layers {
			if by_layer || layer.features.is_empty() {
				if !layer.features.is_empty() && self.expr.evaluate(coord, &layer.name, &GeoProperties::new()) {
					chosen_layers.push(layer);
				} else {
					other_layers.push(layer);
				}
				continue;
			}

			if layer.features.iter().all(|f| f.position.is_none()) {
				for (position, feature) in (0..).zip(&mut layer.features) {
					feature.position = Some(position);
				}
				split.numbered.insert(layer.name.clone());
			}
			split.by_feature.insert(layer.name.clone());

			let mut chosen = Vec::new();
			let mut others = Vec::new();
			for feature in std::mem::take(&mut layer.features) {
				let properties = layer.decode_tag_ids(&feature.tag_ids)?;
				if self.expr.evaluate(coord, &layer.name, &properties) {
					chosen.push(feature);
				} else {
					others.push(feature);
				}
			}
			// Both parts keep the layer's property tables, which their tag ids index.
			if !chosen.is_empty() {
				let mut part = layer.clone();
				part.features = chosen;
				chosen_layers.push(part);
			}
			if !others.is_empty() {
				layer.features = others;
				other_layers.push(layer);
			}
		}

		Ok((VectorTile::new(chosen_layers), VectorTile::new(other_layers), split))
	}
}

/// Which layers [`Operation::split`] split feature by feature.
#[derive(Default)]
struct Split {
	/// Every layer split by feature: its parts are sorted back into order.
	by_feature: HashSet<String>,
	/// The ones numbered by this branch, whose numbers are cleared once sorted.
	numbered: HashSet<String>,
}

/// Puts the halves of a tile back together, layer by layer.
///
/// Layers come out in the order the tile had them, followed by any a branch
/// created. A layer split by feature is sorted back into its original order,
/// with features a branch created after the others.
fn merge(order: &[String], halves: Vec<VectorTile>, split: &Split) -> Result<VectorTile> {
	let mut names = order.to_vec();
	let mut parts: Vec<VectorTileLayer> = Vec::new();
	for half in halves {
		for layer in half.layers {
			if !names.contains(&layer.name) {
				names.push(layer.name.clone());
			}
			parts.push(layer);
		}
	}

	let mut layers = Vec::with_capacity(names.len());
	for name in &names {
		let (these, rest): (Vec<_>, Vec<_>) = parts.into_iter().partition(|layer| &layer.name == name);
		parts = rest;
		let mut these = these.into_iter();
		let Some(mut layer) = these.next() else {
			continue;
		};
		for part in these {
			ensure!(
				part.extent.unwrap_or(4096) == layer.extent.unwrap_or(4096),
				"the branches gave layer {name:?} different extents, {} and {}, so its halves cannot be put together",
				layer.extent.unwrap_or(4096),
				part.extent.unwrap_or(4096)
			);
			layer.add_from_layer(part)?;
		}
		if split.by_feature.contains(name) {
			// Stable, so features without a position keep the order they came in.
			layer.features.sort_by_key(|f| f.position.unwrap_or(u32::MAX));
			// The parts carried the whole layer's property tables. Rebuilt, they
			// hold only what the features left use.
			layer.filter_map_properties(Some)?;
		}
		if split.numbered.contains(name) {
			for feature in &mut layer.features {
				feature.position = None;
			}
		}
		layers.push(layer);
	}
	Ok(VectorTile::new(layers))
}

impl TileTransform for Operation {
	const TAG: &'static str = "vector_branch";

	fn update_metadata(&self, metadata: &mut TileSourceMetadata) {
		if let Some(pyramid) = &self.pyramid {
			metadata.set_tile_pyramid(pyramid.clone());
		}
	}

	fn update_tilejson(&self, tilejson: &mut TileJSON) {
		tilejson.clone_from(&self.tilejson);
	}

	#[context("Failed to run vector_branch")]
	fn run(&self, coord: &TileCoord, tile: Tile) -> Result<Option<Tile>> {
		// One answer for the whole tile: it goes to one branch as it arrived.
		if self.expr.scope() == ExprScope::Tile {
			let branch = if self.expr.evaluate(coord, "", &GeoProperties::new()) {
				&self.then
			} else {
				&self.otherwise
			};
			return branch.run(coord, tile);
		}

		let format = tile.format();
		let vector = tile.into_vector()?;
		let order: Vec<String> = vector.layers.iter().map(|layer| layer.name.clone()).collect();
		let (chosen, others, split) = self.split(coord, vector)?;

		let mut halves = Vec::with_capacity(2);
		for (branch, half) in [(&self.then, chosen), (&self.otherwise, others)] {
			if half.layers.is_empty() {
				continue;
			}
			if let Some(tile) = branch.run(coord, Tile::from_vector(half, format)?)? {
				halves.push(tile.into_vector()?);
			}
		}

		let merged = merge(&order, halves, &split)?;
		if merged.layers.is_empty() {
			Ok(None)
		} else {
			Ok(Some(Tile::from_vector(merged, format)?))
		}
	}
}

crate::operations::macros::define_transform_factory!("vector_branch", Args, Operation, requires: Vector, per_tile);

#[cfg(test)]
mod tests {
	use std::fmt::Write;

	use pretty_assertions::assert_eq;
	use versatiles_core::TilePyramid;

	use super::*;
	use crate::{helpers::dummy_vector_source::DummyVectorSource, vpl::parse_vpl};

	const POI: &[&[(&str, &str)]] = &[
		&[("id", "1"), ("kind", "a"), ("name", "one")],
		&[("id", "2"), ("kind", "b"), ("name", "two")],
		&[("id", "3"), ("kind", "a"), ("name", "three")],
		&[("id", "4"), ("kind", "b"), ("name", "four")],
	];
	const ROADS: &[&[(&str, &str)]] = &[&[("id", "5"), ("kind", "a"), ("name", "five")]];

	fn source() -> DummyVectorSource {
		DummyVectorSource::new(&[("poi", POI), ("roads", ROADS)], Some(TilePyramid::new_full_up_to(8)))
	}

	/// `vpl`, a chain of transforms, built onto the dummy source.
	async fn build(vpl: &str) -> Result<Box<dyn TileSource>> {
		let factory = PipelineFactory::new_dummy();
		let mut operation: Box<dyn TileSource> = Box::new(source());
		for node in parse_vpl(vpl)?.pipeline {
			operation = factory.tran_operation_from_node(node, operation).await?;
		}
		Ok(operation)
	}

	async fn tile_at(vpl: &str, coord: TileCoord) -> Result<Option<VectorTile>> {
		build(vpl).await?.tile(&coord).await?.map(Tile::into_vector).transpose()
	}

	async fn tile(vpl: &str) -> Result<VectorTile> {
		Ok(tile_at(vpl, TileCoord::new(3, 0, 0)?).await?.expect("a tile"))
	}

	/// Each layer's name with a description of each of its features.
	type Layers = Vec<(String, Vec<String>)>;

	/// Per layer, each feature as its `id` followed by the `name` and `kind` it still has.
	fn describe(tile: &VectorTile) -> Layers {
		tile
			.layers
			.iter()
			.map(|layer| {
				let features = layer
					.to_features()
					.unwrap()
					.into_iter()
					.map(|feature| {
						let mut text = feature.properties.get("id").unwrap().to_string();
						for key in ["name", "kind"] {
							if let Some(value) = feature.properties.get(key) {
								let _ = write!(text, " {key}={value}");
							}
						}
						text
					})
					.collect();
				(layer.name.clone(), features)
			})
			.collect()
	}

	fn layer(name: &str, features: &[&str]) -> (String, Vec<String>) {
		(name.to_string(), features.iter().map(ToString::to_string).collect())
	}

	#[tokio::test]
	async fn the_then_branch_changes_only_the_chosen_features_and_order_is_kept() -> Result<()> {
		let tile = tile(r#"vector_branch expr="kind == 'a'" [ vector_filter_properties regex="/name$" ]"#).await?;
		assert_eq!(
			describe(&tile),
			[
				layer(
					"poi",
					&["1 kind=a", "2 name=two kind=b", "3 kind=a", "4 name=four kind=b"]
				),
				layer("roads", &["5 kind=a"]),
			]
		);
		Ok(())
	}

	#[tokio::test]
	async fn the_else_branch_changes_the_others() -> Result<()> {
		let tile = tile(
			r#"vector_branch expr="kind == 'a'" [ vector_filter_properties regex="/name$", vector_filter_properties regex="/kind$" ]"#,
		)
		.await?;
		assert_eq!(
			describe(&tile),
			[
				layer("poi", &["1 kind=a", "2 name=two", "3 kind=a", "4 name=four"]),
				layer("roads", &["5 kind=a"]),
			]
		);
		Ok(())
	}

	#[tokio::test]
	async fn a_branch_can_drop_features() -> Result<()> {
		let tile =
			tile(r#"vector_branch expr="kind == 'a'" [ vector_filter_features layer=["poi"] expr="false" ]"#).await?;
		assert_eq!(
			describe(&tile),
			[
				layer("poi", &["2 name=two kind=b", "4 name=four kind=b"]),
				layer("roads", &["5 name=five kind=a"]),
			]
		);
		Ok(())
	}

	#[tokio::test]
	async fn layers_keep_their_order_whichever_branch_they_take() -> Result<()> {
		let tile = tile(r#"vector_branch expr="$layer == 'roads'" [ vector_filter_properties regex="/kind$" ]"#).await?;
		assert_eq!(
			describe(&tile),
			[
				layer(
					"poi",
					&[
						"1 name=one kind=a",
						"2 name=two kind=b",
						"3 name=three kind=a",
						"4 name=four kind=b"
					]
				),
				layer("roads", &["5 name=five"]),
			]
		);
		Ok(())
	}

	#[tokio::test]
	async fn a_whole_layer_can_be_dropped() -> Result<()> {
		let tile = tile(r#"vector_branch expr="$layer == 'poi'" [ vector_filter_layers filter=["poi"] ]"#).await?;
		assert_eq!(describe(&tile), [layer("roads", &["5 name=five kind=a"])]);
		Ok(())
	}

	/// An expression reading only `$zoom` sends the tile to one branch as it arrived.
	#[tokio::test]
	async fn a_zoom_only_expression_passes_tiles_through_unopened() -> Result<()> {
		const VPL: &str =
			r#"vector_branch expr="$zoom >= 5" [ vector_filter_features layer=["poi","roads"] expr="false" ]"#;
		assert!(tile_at(VPL, TileCoord::new(6, 0, 0)?).await?.is_none());

		let coord = TileCoord::new(3, 1, 2)?;
		let through = build(VPL).await?.tile(&coord).await?;
		assert_eq!(through, source().tile(&coord).await?);
		Ok(())
	}

	/// The inner branch must not renumber what the outer one numbered, or the
	/// outer one could not restore the order of the whole layer.
	#[tokio::test]
	async fn nested_branches_keep_the_order_of_the_whole_layer() -> Result<()> {
		let tile = tile(
			r#"vector_branch expr="id != '1'" [ vector_branch expr="kind == 'a'" [ vector_filter_properties regex="/name$" ] ]"#,
		)
		.await?;
		assert_eq!(
			describe(&tile),
			[
				layer(
					"poi",
					&[
						"1 name=one kind=a",
						"2 name=two kind=b",
						"3 kind=a",
						"4 name=four kind=b"
					]
				),
				layer("roads", &["5 kind=a"]),
			]
		);
		Ok(())
	}

	#[tokio::test]
	async fn no_position_is_left_on_a_feature() -> Result<()> {
		let tile = tile(r#"vector_branch expr="kind == 'a'" [ vector_repair ]"#).await?;
		for layer in &tile.layers {
			assert!(layer.features.iter().all(|f| f.position.is_none()), "{}", layer.name);
		}
		Ok(())
	}

	/// The point of `vector_branch`: what `vector_mask layer=` does, without the
	/// parameter.
	#[tokio::test]
	async fn masking_one_layer_in_a_branch_is_what_vector_mask_layer_does() -> Result<()> {
		// The dummy points sit in the top left corner of each tile, so this keeps
		// them in the tiles at the top left of the world and nowhere else.
		let dir = tempfile::tempdir()?;
		let mask = dir.path().join("mask.geojson");
		std::fs::write(
			&mask,
			r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[-180,60],[-90,60],[-90,85.06],[-180,85.06],[-180,60]]]}}]}"#,
		)?;
		let mask = mask.display();

		let by_parameter = build(&format!(r#"vector_mask geojson="{mask}" layer=["poi"]"#)).await?;
		let by_branch = build(&format!(
			r#"vector_branch expr="$layer == 'poi'" [ vector_mask geojson="{mask}" ]"#
		))
		.await?;

		for coord in [
			TileCoord::new(1, 0, 0)?,
			TileCoord::new(1, 1, 1)?,
			TileCoord::new(3, 0, 0)?,
			TileCoord::new(3, 5, 2)?,
		] {
			let describe_at = |tile: Option<Tile>| -> Result<Option<Layers>> {
				Ok(tile.map(Tile::into_vector).transpose()?.as_ref().map(describe))
			};
			assert_eq!(
				describe_at(by_branch.tile(&coord).await?)?,
				describe_at(by_parameter.tile(&coord).await?)?,
				"at {coord:?}"
			);
		}

		// And the comparison is not between two tiles the mask left alone.
		let layers_at = |tile: Option<Tile>| -> Result<Vec<String>> {
			let tile = tile.expect("a tile").into_vector()?;
			Ok(tile.layers.iter().map(|layer| layer.name.clone()).collect())
		};
		assert_eq!(
			layers_at(by_branch.tile(&TileCoord::new(1, 0, 0)?).await?)?,
			["poi", "roads"]
		);
		assert_eq!(layers_at(by_branch.tile(&TileCoord::new(1, 1, 1)?).await?)?, ["roads"]);
		Ok(())
	}

	async fn build_error(vpl: &str) -> String {
		format!("{:#}", build(vpl).await.expect_err("should not build"))
	}

	#[tokio::test]
	async fn an_operation_reading_other_tiles_is_refused() {
		let message = build_error(r#"vector_branch expr="kind == 'a'" [ vector_overzoom level_max=10 ]"#).await;
		assert!(
			message.contains("'vector_overzoom' cannot run inside `vector_branch`"),
			"{message}"
		);
	}

	#[tokio::test]
	async fn a_read_operation_is_refused() {
		let message = build_error(r#"vector_branch expr="kind == 'a'" [ from_debug format=mvt ]"#).await;
		assert!(message.contains("'from_debug' is a read operation"), "{message}");
	}

	#[tokio::test]
	async fn a_raster_operation_is_refused() {
		let message = build_error(r#"vector_branch expr="kind == 'a'" [ raster_levels brightness=10 ]"#).await;
		assert!(message.contains("needs raster tiles"), "{message}");
	}

	#[tokio::test]
	async fn a_third_branch_is_refused() {
		let message =
			build_error(r#"vector_branch expr="kind == 'a'" [ vector_repair, vector_repair, vector_repair ]"#).await;
		assert!(message.contains("not 3"), "{message}");
	}

	/// Built as a whole pipeline, so the path starts at the root: the branch is
	/// node 1, and the failing step is node 1 of its first source.
	#[tokio::test]
	async fn an_error_names_the_step_in_the_branch() {
		let factory = PipelineFactory::new_dummy();
		let error = factory
			.operation_from_vpl(
				r#"from_debug format=mvt | vector_branch expr="kind == 'a'" [ vector_repair | vector_filter_layers nonsense=1 ]"#,
			)
			.await
			.expect_err("should not build");
		let message = format!("{error:#}");
		assert!(
			message.contains("node 1, source 0, node 1 (vector_filter_layers)"),
			"{message}"
		);
	}
}
