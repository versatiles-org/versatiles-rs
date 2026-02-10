use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{GeoBBox, GeoCenter, TileBBox, TileJSON, TileSchema, TileStream};
use versatiles_derive::context;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Update metadata, see also <https://github.com/mapbox/tilejson-spec/tree/master/3.0.0>
struct Args {
	/// Attribution text.
	attribution: Option<String>,
	/// Geographic bounding box [west, south, east, north].
	bounds: Option<[f64; 4]>,
	/// Default center [longitude, latitude, zoom].
	center: Option<[f64; 3]>,
	/// Description text.
	description: Option<String>,
	/// Fill zoom level.
	fillzoom: Option<u8>,
	/// Legend text.
	legend: Option<String>,
	/// Name text.
	name: Option<String>,
	/// Tile schema, allowed values: "rgb", "rgba", "dem/mapbox", "dem/terrarium", "dem/versatiles", "openmaptiles", "shortbread@1.0", "other", "unknown"
	schema: Option<TileSchema>,
}

#[derive(Debug)]
struct Operation {
	source: Box<dyn TileSource>,
	tilejson: TileJSON,
}

impl Operation {
	#[context("Building meta_update operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let mut tilejson = source.tilejson().clone();

		if let Some(attribution) = args.attribution {
			tilejson.set_string("attribution", &attribution)?;
		}

		if let Some(bounds) = args.bounds {
			tilejson.bounds = Some(GeoBBox::try_from(&bounds)?);
		}

		if let Some(center) = args.center {
			tilejson.center = Some(GeoCenter::try_from(center.to_vec())?);
		}

		if let Some(description) = args.description {
			tilejson.set_string("description", &description)?;
		}

		if let Some(fillzoom) = args.fillzoom {
			tilejson.set_byte("fillzoom", fillzoom)?;
		}

		if let Some(legend) = args.legend {
			tilejson.set_string("legend", &legend)?;
		}

		if let Some(name) = args.name {
			tilejson.set_string("name", &name)?;
		}

		if let Some(schema) = args.schema {
			tilejson.tile_schema = Some(schema);
		}

		Ok(Self { source, tilejson })
	}
}

#[async_trait]
impl TileSource for Operation {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("meta_update", self.source.source_type())
	}

	fn metadata(&self) -> &TileSourceMetadata {
		self.source.metadata()
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		self.source.get_tile_stream(bbox).await
	}
}

crate::operations::macros::define_transform_factory!("meta_update", Args, Operation);

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	use super::*;
	use crate::PipelineFactory;

	fn get_str(o: &TileJSON, k: &str) -> Option<String> {
		o.as_object().get_string(k).ok().flatten()
	}
	fn get_num(o: &TileJSON, k: &str) -> Option<f64> {
		o.as_object().get_number(k).ok().flatten()
	}

	#[tokio::test]
	async fn test_meta_update_sets_fields_and_preserves_others() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
            .operation_from_vpl(
                "from_debug format=mvt \
                 | filter bbox=[0,0,10,10] level_min=2 level_max=7 \
                 | meta_update name=\"Test Layer\" description=\"My desc\" attribution=\"CC-BY\" \
                   bounds=[-10,-5,10,5] center=[1.5,2.5,8] fillzoom=12 legend=\"My legend\" \
                   schema=\"shortbread@1.0\"",
            )
            .await?;

		let tj = op.tilejson();

		// Updated keys are present
		assert_eq!(get_str(tj, "name").as_deref(), Some("Test Layer"));
		assert_eq!(get_str(tj, "description").as_deref(), Some("My desc"));
		assert_eq!(get_str(tj, "attribution").as_deref(), Some("CC-BY"));
		assert_eq!(get_num(tj, "fillzoom"), Some(12.0));
		assert_eq!(get_str(tj, "legend").as_deref(), Some("My legend"));

		// Bounds
		assert_eq!(tj.bounds.unwrap().as_tuple(), (-10.0, -5.0, 10.0, 5.0));

		// Center
		let center = tj.center.unwrap();
		assert_eq!(center.0, 1.5);
		assert_eq!(center.1, 2.5);
		assert_eq!(center.2, 8);

		// Tile Content was parsed into typed field
		assert_eq!(tj.tile_schema, Some(TileSchema::try_from("shortbread@1.0")?));

		// Pre-existing zooms from the filter should remain intact
		assert_eq!(tj.as_object().get_number("minzoom")?.unwrap(), 2.0);
		assert_eq!(tj.as_object().get_number("maxzoom")?.unwrap(), 7.0);
		Ok(())
	}

	#[tokio::test]
	async fn test_meta_update_is_noop_when_no_args() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op1 = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[-5,-5,5,5] level_min=1 level_max=4")
			.await?;
		let op2 = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[-5,-5,5,5] level_min=1 level_max=4 | meta_update")
			.await?;

		let t1 = op1.tilejson().clone();
		let t2 = op2.tilejson().clone();

		// With no args, the operation should not alter TileJSON
		assert_eq!(t1, t2);
		Ok(())
	}
}
