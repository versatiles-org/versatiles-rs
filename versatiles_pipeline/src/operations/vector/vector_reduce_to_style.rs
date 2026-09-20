use anyhow::Result;
use versatiles_container::TileSource;
use versatiles_derive::context;

use crate::{
	PipelineFactory,
	helpers::location::FilePath,
	reduce::{from_style, operations},
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
/// It expands to `vector_filter_layers`, one `vector_filter_features` per layer
/// that needs one, and `vector_filter_properties` — so the same reduction can be
/// written by hand, and `versatiles reduce --print` shows what was derived.
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

/// Builds the filter operations a style implies and folds them onto the source.
///
/// There is no `Runner` here and no per-tile work: this operation *is* the three filter operations,
/// chosen by reading a style. Rendering to real `VPLNode`s and building them through the factory —
/// rather than filtering directly — means the tested operations do the work, the expansion is the
/// same thing a user could have written by hand, and there is only one implementation of what
/// `has(props.k)` means.
struct Operation {}

impl Operation {
	#[context("Building vector_reduce_to_style operation in VPL node {:?}", vpl_node.name)]
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &PipelineFactory,
	) -> Result<Box<dyn TileSource>> {
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

		let mut operation = source;
		for node in operations(&requirement) {
			operation = factory.tran_operation_from_node(node, operation).await?;
		}

		Ok(operation)
	}
}

// The `define_transform_factory!` macro is not used here: it boxes what `build` returns, and this
// `build` already returns a `Box<dyn TileSource>` — the last of the operations it folded — rather
// than a type of its own.
pub struct Factory {}

impl crate::factory::OperationFactoryTrait for Factory {
	fn docs(&self) -> String {
		Args::docs()
	}
	#[cfg(feature = "codegen")]
	fn doc_summary(&self) -> String {
		Args::doc_summary()
	}
	#[cfg(feature = "codegen")]
	fn doc_details(&self) -> String {
		Args::doc_details()
	}
	fn tag_name(&self) -> &str {
		"vector_reduce_to_style"
	}
	fn field_metadata(&self) -> Vec<crate::vpl::VPLFieldMeta> {
		Args::field_metadata()
	}
}

#[async_trait::async_trait]
impl crate::factory::TransformOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn TileSource>> {
		crate::factory::check_compatibility(
			<Self as crate::factory::TransformOperationFactoryTrait>::compatibility(self, source.as_ref()).await,
		)?;
		Operation::build(vpl_node, source, factory).await
	}

	async fn compatibility(&self, source: &dyn TileSource) -> crate::factory::Compatibility {
		crate::factory::require_tile_type(source, versatiles_core::TileType::Vector, "vector_reduce_to_style")
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;
	use versatiles_core::TileBBox;

	use super::*;

	/// A style written to a temporary file, since the argument is a path.
	fn style_file(dir: &tempfile::TempDir, json: &str) -> String {
		let path = dir.path().join("style.json");
		std::fs::write(&path, json).unwrap();
		path.to_str().unwrap().to_string()
	}

	/// The properties surviving in `from_debug`'s tiles, as `layer/property`.
	async fn surviving(style: &str) -> Result<Vec<String>> {
		let dir = tempfile::tempdir().unwrap();
		let path = style_file(&dir, style);
		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl(&format!(
				"from_debug format=mvt | vector_reduce_to_style style=\"{path}\""
			))
			.await?;

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
		let path = style_file(&dir, r#"{"layers":[]}"#);
		let factory = PipelineFactory::new_dummy();
		let err = factory
			.operation_from_vpl(&format!(
				"from_debug format=mvt | vector_reduce_to_style style=\"{path}\""
			))
			.await
			.unwrap_err();
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
		let path = style_file(
			&dir,
			r#"{"layers":[{"id":"a","source-layer":"debug_x","filter":["has","char"],
				"paint":{"fill-color":["get","char"]}}]}"#,
		);
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_debug format=mvt | vector_reduce_to_style style=\"{path}\""
			))
			.await?;
		let tiles = op.tile_stream(TileBBox::new_full(1)?).await?.to_vec().await;
		assert_tiles_valid(tiles);
		Ok(())
	}
}
