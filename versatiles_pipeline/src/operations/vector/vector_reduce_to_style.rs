use std::collections::HashSet;

use anyhow::Result;
use regex::Regex;
use versatiles_container::TileSource;
use versatiles_core::{TileCoord, TileJSON};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;

use crate::{
	PipelineFactory,
	helpers::location::FilePath,
	operations::{
		transform::{AsTileTransform, TransformOp, VectorTransform},
		vector::vector_filter_features,
	},
	reduce::{self, from_style},
	vpl::VPLNode,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reduces vector tiles to what one MapLibre style actually draws.
///
/// Reads the style, works out which source-layers it draws, which properties it
/// reads and which features it filters to, then drops everything else. A
/// Shortbread tileset reduced to a style that only ever reads `name` loses every
/// `name_*` translation, and a layer drawn as plain geometry loses all of its
/// properties while keeping its shapes.
///
/// The same reduction can be written by hand as `vector_filter_layers`, one
/// `vector_filter_features` per layer, and `vector_filter_properties`. This does
/// it in one pass instead: chaining twenty operations would decode and re-encode
/// every tile twenty times to touch one layer each time.
///
/// Zoom levels are clamped to the pyramid of the source it wraps. That matters:
/// MapLibre overzooms, so a style drawing addresses from z17 against a tileset
/// that stops at z14 must keep them in the z14 tiles, not drop them.
///
/// **The reduction may keep features the style never draws; it never drops one
/// it does.** Anything in the style that cannot be read widens toward keeping.
struct Args {
	/// Path to the MapLibre style JSON the tileset should be reduced to.
	#[vpl(accepts = "json")]
	style: FilePath,
}

/// Everything a style asks of a tileset, compiled once and applied per tile.
///
/// The three stages are the three operations a hand-written reduction would chain, collapsed into
/// one pass over the decoded tile. Collapsing them is not only about the decode/encode cost: a
/// chain of twenty nested sources also nests twenty async stream adapters, and on a Windows main
/// thread — 1 MB of stack, against 8 on Linux — a realistic style overflowed it.
struct Runner {
	/// Source-layers the style draws. Everything else goes.
	layers: HashSet<String>,
	/// One compiled predicate per layer that needs one, reusing `vector_filter_features`' runner so
	/// there is a single implementation of what an expression means.
	filters: Vec<vector_filter_features::Runner>,
	/// Matches `layer/property` for the properties to keep.
	properties: Regex,
}

impl std::fmt::Debug for Runner {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Runner")
			.field("layers", &self.layers)
			.field("filters", &self.filters.len())
			.field("properties", &self.properties.as_str())
			.finish()
	}
}

impl Runner {
	/// Compiles a requirement into the three stages.
	fn from_requirement(requirement: &reduce::Requirement) -> Result<Self> {
		let filters = reduce::feature_expressions(requirement)
			.into_iter()
			.map(|(layer, expression)| vector_filter_features::Runner::new([layer], &expression))
			.collect::<Result<Vec<_>>>()?;

		Ok(Self {
			layers: reduce::layer_names(requirement).into_iter().collect(),
			filters,
			properties: Regex::new(&reduce::property_regex(requirement))?,
		})
	}
}

impl VectorTransform for Runner {
	const TAG: &'static str = "vector_reduce_to_style";

	#[context("Failed to reduce a tile to the style")]
	fn run(&self, coord: &TileCoord, mut tile: VectorTile) -> Result<Option<VectorTile>> {
		// Layers first: it is the cheapest discriminator, and every later stage then walks less.
		tile.layers.retain(|layer| self.layers.contains(&layer.name));

		// Features before properties, and that ordering is a correctness requirement rather than a
		// preference: the predicates read properties through `has(props.k)`, so stripping
		// properties first would make every guard false and delete the features they keep.
		for filter in &self.filters {
			match filter.filter(coord, tile)? {
				Some(filtered) => tile = filtered,
				// The filter emptied the tile, so nothing downstream has anything to do.
				None => return Ok(None),
			}
		}

		for layer in &mut tile.layers {
			let name = layer.name.clone();
			layer.filter_map_properties(|mut properties| {
				properties.retain(|key, _| self.properties.is_match(&format!("{name}/{key}")));
				Some(properties)
			})?;
		}

		if tile.layers.is_empty() {
			Ok(None)
		} else {
			Ok(Some(tile))
		}
	}

	fn update_tilejson(&self, tilejson: &mut TileJSON) {
		tilejson.vector_layers.0.retain(|name, _| self.layers.contains(name));
		tilejson.vector_layers.iter_mut().for_each(|(name, layer)| {
			layer
				.fields
				.retain(|key, _| self.properties.is_match(&format!("{name}/{key}")));
		});
	}
}

impl Runner {
	#[context("Building vector_reduce_to_style operation in VPL node {:?}", vpl_node.name)]
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &PipelineFactory,
	) -> Result<TransformOp<AsTileTransform<Runner>>> {
		let args = Args::from_vpl_node(&vpl_node)?;

		let location = factory
			.resolve_location(&args.style.to_location())
			.with_context(|| format!("resolving 'style' path {:?}", args.style))?;
		let path = location.to_path_buf()?;
		let json = std::fs::read_to_string(&path).with_context(|| format!("reading 'style' from {}", path.display()))?;

		// The source's own pyramid is the only thing that knows how deep the tiles go, and an
		// unclamped requirement is the one output that can silently destroy data — see the module
		// docs of `reduce::style`. A source that cannot say is refused rather than guessed at.
		let maxzoom = source
			.tile_pyramid()
			.await?
			.level_max()
			.context("the source has no tiles, so there is no maximum zoom to clamp the style's zooms to")?;

		let requirement = from_style(&json, maxzoom).with_context(|| format!("reading style {}", path.display()))?;

		Ok(TransformOp::new(
			source,
			AsTileTransform(Runner::from_requirement(&requirement)?),
			factory.runtime(),
		))
	}
}

crate::operations::macros::define_transform_factory!("vector_reduce_to_style", Args, Runner, requires: Vector);

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;
	use versatiles_core::TileBBox;

	use super::*;

	/// A style written to a temporary file, spelled for embedding in double-quoted VPL.
	///
	/// The backslash doubling is not cosmetic: a double-quoted VPL string processes `\` as an
	/// escape, so a Windows temp path like `C:\Users\RUNNER~1\AppData\…` fails to parse as written.
	/// The operation itself is fine — the CLI builds `VPLNode`s and lets the serializer quote — but
	/// a test that assembles VPL *text* has to escape it, as `vector_update_properties`' tests do.
	fn style_file(dir: &tempfile::TempDir, json: &str) -> String {
		let path = dir.path().join("style.json");
		std::fs::write(&path, json).unwrap();
		path.to_str().unwrap().replace('\\', "\\\\")
	}

	async fn operation(dir: &tempfile::TempDir, style: &str) -> Result<Box<dyn TileSource>> {
		let path = style_file(dir, style);
		PipelineFactory::new_dummy()
			.operation_from_vpl(&format!(
				"from_debug format=mvt | vector_reduce_to_style style=\"{path}\""
			))
			.await
	}

	/// The properties surviving in `from_debug`'s tiles, as `layer/property`.
	async fn surviving(style: &str) -> Result<Vec<String>> {
		let dir = tempfile::tempdir().unwrap();
		let operation = operation(&dir, style).await?;

		let tile = operation
			.tile_stream(TileBBox::new_full(1)?)
			.await?
			.next()
			.await
			.unwrap()
			.1
			.into_vector()?;

		let mut properties: Vec<String> = tile
			.layers
			.iter()
			.flat_map(|layer| {
				let name = layer.name.clone();
				layer.features.iter().flat_map(move |feature| {
					let p = feature.decode_properties(layer).unwrap();
					p.iter().map(|(k, _)| format!("{name}/{k}")).collect::<Vec<_>>()
				})
			})
			.collect();
		properties.sort();
		properties.dedup();
		Ok(properties)
	}

	#[tokio::test]
	async fn it_keeps_only_what_the_style_reads() {
		// `from_debug` draws layers `debug_x`, `debug_y` and `debug_z`, each carrying `char`,
		// `index` and `x`. A style that reads one property of one layer should leave exactly that.
		let properties =
			surviving(r#"{"layers":[{"id":"a","source-layer":"debug_x","paint":{"fill-color":["get","char"]}}]}"#)
				.await
				.unwrap();
		assert_eq!(properties, ["debug_x/char"]);
	}

	#[tokio::test]
	async fn a_layer_drawn_as_geometry_keeps_its_shapes_but_no_properties() {
		let properties = surviving(r##"{"layers":[{"id":"a","source-layer":"debug_x","paint":{"fill-color":"#fff"}}]}"##)
			.await
			.unwrap();
		assert_eq!(properties, Vec::<String>::new());
	}

	#[tokio::test]
	async fn a_token_text_field_is_read_like_any_other_property() {
		// The regression `reduce::style::fields` exists for, proven end to end through the pipeline.
		let properties =
			surviving(r#"{"layers":[{"id":"a","source-layer":"debug_x","layout":{"text-field":"{char}"}}]}"#)
				.await
				.unwrap();
		assert_eq!(properties, ["debug_x/char"]);
	}

	#[tokio::test]
	async fn the_tilejson_loses_the_layers_and_fields_the_style_never_reads() {
		// Whoever reads the reduced container's metadata must not be told about data that is no
		// longer in it.
		let dir = tempfile::tempdir().unwrap();
		let operation = operation(
			&dir,
			r#"{"layers":[{"id":"a","source-layer":"debug_x","paint":{"fill-color":["get","char"]}}]}"#,
		)
		.await
		.unwrap();

		let tilejson = operation.tilejson();
		let layers: Vec<&String> = tilejson.vector_layers.0.keys().collect();
		assert_eq!(layers, ["debug_x"]);
		assert_eq!(
			tilejson.vector_layers.0["debug_x"].fields.keys().collect::<Vec<_>>(),
			["char"]
		);
	}

	#[tokio::test]
	async fn it_is_one_operation_rather_than_a_chain() {
		// The whole point of the collapse: twenty nested sources decode and re-encode every tile
		// twenty times, and on a 1 MB Windows main thread they overflowed the stack.
		let dir = tempfile::tempdir().unwrap();
		let operation = operation(
			&dir,
			r#"{"layers":[
				{"id":"a","source-layer":"debug_x","filter":["has","char"],"paint":{"c":["get","char"]}},
				{"id":"b","source-layer":"debug_y","filter":["has","index"],"paint":{"c":["get","index"]}},
				{"id":"c","source-layer":"debug_z","filter":["has","x"],"paint":{"c":["get","x"]}}
			]}"#,
		)
		.await
		.unwrap();

		// Three layers each with their own predicate, and still a single processor above the source.
		let rendered = operation.source_type().to_string();
		assert_eq!(rendered.matches("vector_reduce_to_style").count(), 1, "got: {rendered}");
		assert!(!rendered.contains("vector_filter_features"), "got: {rendered}");
	}

	#[tokio::test]
	async fn a_path_containing_a_backslash_is_still_found() {
		// Every Windows temp path is full of backslashes, and a double-quoted VPL string reads `\`
		// as an escape — which is how this operation's tests failed on Windows and nowhere else.
		// A backslash is a legal filename character on Unix too, so the hazard is reproducible on
		// every platform rather than only in CI.
		let dir = tempfile::tempdir().unwrap();
		let nested = dir.path().join("a\\b");
		std::fs::create_dir_all(&nested).unwrap();

		let path = nested.join("style.json");
		std::fs::write(
			&path,
			r#"{"layers":[{"id":"a","source-layer":"debug_x","paint":{"fill-color":["get","char"]}}]}"#,
		)
		.unwrap();
		let escaped = path.to_str().unwrap().replace('\\', "\\\\");

		PipelineFactory::new_dummy()
			.operation_from_vpl(&format!(
				"from_debug format=mvt | vector_reduce_to_style style=\"{escaped}\""
			))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn a_missing_style_file_is_reported() {
		let factory = PipelineFactory::new_dummy();
		let err = factory
			.operation_from_vpl("from_debug format=mvt | vector_reduce_to_style style=\"does-not-exist.json\"")
			.await
			.unwrap_err();
		assert!(format!("{err:#}").contains("reading 'style'"), "got: {err:#}");
	}

	#[tokio::test]
	async fn a_style_drawing_no_data_is_refused() {
		let dir = tempfile::tempdir().unwrap();
		let err = operation(&dir, r#"{"layers":[]}"#).await.unwrap_err();
		assert!(format!("{err:#}").contains("draws no source-layer"), "got: {err:#}");
	}

	#[tokio::test]
	async fn the_style_argument_is_required() {
		let factory = PipelineFactory::new_dummy();
		let err = factory
			.operation_from_vpl("from_debug format=mvt | vector_reduce_to_style")
			.await
			.unwrap_err();
		assert!(
			err.chain().any(|e| e.to_string().contains("'style' is required")),
			"got: {err:?}"
		);
	}

	#[tokio::test]
	async fn output_tiles_pass_mvt_validation() -> Result<()> {
		use crate::helpers::assert_tiles_valid;
		let dir = tempfile::tempdir().unwrap();
		let op = operation(
			&dir,
			r#"{"layers":[{"id":"a","source-layer":"debug_x","filter":["has","char"],
				"paint":{"fill-color":["get","char"]}}]}"#,
		)
		.await?;
		let tiles = op.tile_stream(TileBBox::new_full(1)?).await?.to_vec().await;
		assert_tiles_valid(tiles);
		Ok(())
	}
}
