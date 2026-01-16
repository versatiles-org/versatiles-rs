/// Generates Factory boilerplate for transform operations.
///
/// Usage: `define_transform_factory!("raster_flatten", Args, Operation);`
///
/// This generates a `Factory` struct that implements `OperationFactoryTrait`
/// and `TransformOperationFactoryTrait`, delegating to `Operation::build`.
macro_rules! define_transform_factory {
	($tag:literal, $args:ty, $op:ty) => {
		pub struct Factory {}

		impl $crate::factory::OperationFactoryTrait for Factory {
			fn get_docs(&self) -> String {
				<$args>::get_docs()
			}
			fn get_tag_name(&self) -> &str {
				$tag
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
			fn get_docs(&self) -> String {
				<$args>::get_docs()
			}
			fn get_tag_name(&self) -> &str {
				$tag
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
