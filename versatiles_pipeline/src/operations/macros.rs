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
