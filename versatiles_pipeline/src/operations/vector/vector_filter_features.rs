use crate::{
	PipelineFactory,
	factory::{OperationFactoryTrait, TransformOperationFactoryTrait},
	operations::vector::traits::{RunnerTrait, build_transform},
	vpl::VPLNode,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use cel_interpreter::Program;
use std::collections::HashSet;
use versatiles_container::TileSource;
use versatiles_core::TileJSON;
use versatiles_derive::context;
use versatiles_geometry::{geo::GeoProperties, vector_tile::VectorTile};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Drops vector features in selected layers that do not satisfy a boolean CEL expression.
/// Features in layers outside `layer` pass through untouched.
///
/// # Examples
///
///   vector_filter_features layer=["place"] expr="name == 'Berlin'"
///   vector_filter_features layer=["poi"]   expr="population >= 1000"
///   vector_filter_features layer=["road"]  expr="highway in ['primary','secondary']"
///   vector_filter_features layer=["place"] expr="name.matches('^St\\.')"
///   vector_filter_features layer=["poi"]   expr="has(name) && name != ''"
///   vector_filter_features layer=["addr"]  expr="props['addr:street'] == 'Hauptstr.'"
struct Args {
	/// Layers the expression applies to, as a VPL array of strings.
	/// Features in all other layers are left unchanged.
	/// Example: `layer=["poi","place"]`.
	layer: Vec<String>,

	/// CEL (Common Expression Language) boolean expression.
	/// Feature properties are available as `props["key"]`; properties whose names are
	/// valid CEL identifiers (letters, digits, underscore) are also exposed as top-level
	/// identifiers. Missing keys resolve to null; use `has(name)` for explicit presence
	/// checks. See `versatiles help` for a CEL operator cheat-sheet.
	expr: String,
}

#[derive(Debug)]
struct Runner {
	layer_set: HashSet<String>,
	program: Program,
}

impl Runner {
	pub fn from_args(args: &Args) -> Result<Self> {
		let program =
			Program::compile(&args.expr).map_err(|e| anyhow!("Failed to compile CEL expression `{}`: {e}", args.expr))?;

		Ok(Self {
			layer_set: args.layer.iter().cloned().collect(),
			program,
		})
	}

	// TODO (step 3): build a cel_interpreter::Context from `props` (exposing both `props["key"]`
	// map access and identifier-safe top-level names, with missing keys resolving to null),
	// execute `self.program`, and return `Ok(true)` only when it evaluates to `Value::Bool(true)`.
	fn evaluate(&self, _props: &GeoProperties) -> Result<bool> {
		let _ = &self.program;
		Ok(false)
	}
}

impl RunnerTrait for Runner {
	#[context("Failed to run vector_filter_features")]
	fn run(&self, mut tile: VectorTile) -> Result<Option<VectorTile>> {
		tile.layers.retain_mut(|layer| {
			if !self.layer_set.contains(&layer.name) {
				return true;
			}
			let _ = layer.filter_map_properties(|props| match self.evaluate(&props) {
				Ok(true) => Some(props),
				_ => None,
			});
			!layer.features.is_empty()
		});

		if tile.layers.is_empty() {
			Ok(None)
		} else {
			Ok(Some(tile))
		}
	}

	fn update_tilejson(&self, _tilejson: &mut TileJSON) {}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn docs(&self) -> String {
		Args::docs()
	}
	fn tag_name(&self) -> &str {
		"vector_filter_features"
	}
	#[cfg(feature = "codegen")]
	fn field_metadata(&self) -> Vec<crate::vpl::VPLFieldMeta> {
		Args::field_metadata()
	}
}

#[async_trait]
impl TransformOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		_factory: &'a PipelineFactory,
	) -> Result<Box<dyn TileSource>> {
		let args = Args::from_vpl_node(&vpl_node)?;
		build_transform::<Runner>(source, Runner::from_args(&args)?).await
	}
}
