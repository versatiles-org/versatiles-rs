/// Generates Factory boilerplate for transform operations.
///
/// Usage: `define_transform_factory!("raster_flatten", Args, Operation);`
///
/// Add `, requires: Raster` (or `Vector`) when the operation only applies to one
/// kind of tile. That generates a `compatibility()` that refuses anything else,
/// so a picker can leave the operation out and say why. Omit it when nothing
/// about the source can rule the operation out — `filter` and `meta_update`
/// apply to anything, and `dem_*` cannot tell terrarium elevation from a
/// photograph, so both keep the default `Fits`.
///
/// This generates a `Factory` struct that implements `OperationFactoryTrait`
/// and `TransformOperationFactoryTrait`, delegating to `Operation::build`.
/// `build` consults `compatibility()` first and refuses with its reason, so an
/// operation that a picker would leave out cannot be built by hand either.
macro_rules! define_transform_factory {
	($tag:literal, $args:ty, $op:ty) => {
		$crate::operations::macros::define_transform_factory!(@impl $tag, $args, $op,);
	};
	($tag:literal, $args:ty, $op:ty, requires: $tile_type:ident) => {
		$crate::operations::macros::define_transform_factory!(@impl $tag, $args, $op,
			async fn compatibility(
				&self,
				source: &dyn versatiles_container::TileSource,
			) -> $crate::factory::Compatibility {
				$crate::factory::require_tile_type(source, versatiles_core::TileType::$tile_type, $tag)
			}
		);
	};
	(@impl $tag:literal, $args:ty, $op:ty, $($compatibility:tt)*) => {
		pub struct Factory {}

		impl $crate::factory::OperationFactoryTrait for Factory {
			fn docs(&self) -> String {
				<$args>::docs()
			}
			#[cfg(feature = "codegen")]
			fn doc_summary(&self) -> String {
				<$args>::doc_summary()
			}
			#[cfg(feature = "codegen")]
			fn doc_details(&self) -> String {
				<$args>::doc_details()
			}
			fn tag_name(&self) -> &str {
				$tag
			}
			fn field_metadata(&self) -> Vec<$crate::vpl::VPLFieldMeta> {
				<$args>::field_metadata()
			}
		}

		#[async_trait::async_trait]
		impl $crate::factory::TransformOperationFactoryTrait for Factory {
			async fn build<'a>(
				&self,
				vpl_node: $crate::vpl::VPLNode,
				source: Box<dyn versatiles_container::TileSource>,
				factory: &'a $crate::PipelineFactory,
			) -> anyhow::Result<Box<dyn versatiles_container::TileSource>> {
				// The verdict and the build agree by construction rather than by two
				// people remembering. Without this, `compatibility()` was the stricter
				// of the two: `raster_flatten` on an mvt source built successfully and
				// only failed once a tile was pulled through it, where the failure had
				// no way left to be an `Err`.
				$crate::factory::check_compatibility(
					<Self as $crate::factory::TransformOperationFactoryTrait>::compatibility(self, source.as_ref()).await,
				)?;
				<$op>::build(vpl_node, source, factory)
					.await
					.map(|op| Box::new(op) as Box<dyn versatiles_container::TileSource>)
			}

			$($compatibility)*
		}
	};
}

/// Generates Factory boilerplate for read operations.
///
/// Usage: `define_read_factory!("from_container", Args, Operation);`
///
/// This generates a `Factory` struct that implements `OperationFactoryTrait`
/// and `ReadOperationFactoryTrait`, delegating to `Operation::build`.
///
/// `$op` needs an inherent
/// `async fn build(VPLNode, &PipelineFactory) -> Result<Box<dyn TileSource>>`
/// and nothing else — no trait to implement. What it returns need not be `$op`:
/// `from_geo` and `from_csv` both build a
/// [`FeatureTileSource`](crate::helpers::feature_tile_source::FeatureTileSource),
/// and the transform operations return a
/// [`TransformOp`](crate::operations::transform::TransformOp) wrapping
/// themselves.
macro_rules! define_read_factory {
	($tag:literal, $args:ty, $op:ty) => {
		pub struct Factory {}

		impl $crate::factory::OperationFactoryTrait for Factory {
			fn docs(&self) -> String {
				<$args>::docs()
			}
			#[cfg(feature = "codegen")]
			fn doc_summary(&self) -> String {
				<$args>::doc_summary()
			}
			#[cfg(feature = "codegen")]
			fn doc_details(&self) -> String {
				<$args>::doc_details()
			}
			fn tag_name(&self) -> &str {
				$tag
			}
			fn field_metadata(&self) -> Vec<$crate::vpl::VPLFieldMeta> {
				<$args>::field_metadata()
			}
		}

		#[async_trait::async_trait]
		impl $crate::factory::ReadOperationFactoryTrait for Factory {
			async fn build<'a>(
				&self,
				vpl_node: $crate::vpl::VPLNode,
				factory: &'a $crate::PipelineFactory,
			) -> anyhow::Result<Box<dyn versatiles_container::TileSource>> {
				<$op>::build(vpl_node, factory).await
			}
		}
	};
}

pub(crate) use define_read_factory;
pub(crate) use define_transform_factory;
