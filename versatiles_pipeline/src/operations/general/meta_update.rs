use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{GeoBBox, GeoCenter, TileBBox, TileJSON, TilePyramid, TileSchema, TileStream, json::parse_json_str};
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
	/// The `vector_layers` array as a JSON string. It is parsed and validated against the
	/// TileJSON spec before replacing the source's `vector_layers`.
	vector_layers: Option<String>,
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

		if let Some(vector_layers) = args.vector_layers {
			let json = parse_json_str(&vector_layers).context("parsing 'vector_layers' as JSON")?;
			tilejson
				.set_vector_layers(&json)
				.context("validating 'vector_layers'")?;
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

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self.source.tile_pyramid().await
	}

	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("meta_update::tile_stream {bbox:?}");
		self.source.tile_stream(bbox).await
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.source.tile_coord_stream(bbox).await
	}
}

crate::operations::macros::define_transform_factory!("meta_update", Args, Operation);

#[cfg(test)]
mod tests {
	use super::*;
	use crate::PipelineFactory;
	use approx::assert_relative_eq;

	fn get_str(o: &TileJSON, k: &str) -> Option<String> {
		o.as_object().string(k).ok().flatten()
	}
	fn get_num(o: &TileJSON, k: &str) -> Option<f64> {
		o.as_object().number(k).ok().flatten()
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
		assert_relative_eq!(center.0, 1.5);
		assert_relative_eq!(center.1, 2.5);
		assert_eq!(center.2, 8);

		// Tile Content was parsed into typed field
		assert_eq!(tj.tile_schema, Some(TileSchema::try_from("shortbread@1.0")?));

		// Pre-existing zooms from the filter should remain intact
		assert_relative_eq!(tj.as_object().number("minzoom")?.unwrap(), 2.0);
		assert_relative_eq!(tj.as_object().number("maxzoom")?.unwrap(), 7.0);
		Ok(())
	}

	#[tokio::test]
	async fn test_meta_update_sets_vector_layers_from_json() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_debug format=mvt | meta_update \
				 vector_layers='[{\"id\":\"place_labels\",\"minzoom\":0,\"maxzoom\":14,\
				 \"fields\":{\"name\":\"String\",\"population\":\"Number\"}}]'",
			)
			.await?;

		let layers = &op.tilejson().vector_layers;
		let place_labels = layers.find("place_labels").expect("place_labels should be set");
		assert_eq!(place_labels.fields.get("name").map(String::as_str), Some("String"));
		assert_eq!(place_labels.fields.get("population").map(String::as_str), Some("Number"));
		assert_eq!(place_labels.minzoom, Some(0));
		assert_eq!(place_labels.maxzoom, Some(14));
		Ok(())
	}

	#[tokio::test]
	async fn test_meta_update_rejects_malformed_json() {
		let factory = PipelineFactory::new_dummy();
		let err = factory
			.operation_from_vpl("from_debug format=mvt | meta_update vector_layers='[{not valid json'")
			.await
			.unwrap_err();
		assert!(
			format!("{err:#}").contains("parsing 'vector_layers' as JSON"),
			"got: {err:#}"
		);
	}

	#[tokio::test]
	async fn test_meta_update_rejects_invalid_vector_layers() {
		let factory = PipelineFactory::new_dummy();
		// Structurally valid JSON, but a layer entry is missing the required `id`.
		let err = factory
			.operation_from_vpl("from_debug format=mvt | meta_update vector_layers='[{\"fields\":{}}]'")
			.await
			.unwrap_err();
		assert!(
			format!("{err:#}").contains("validating 'vector_layers'"),
			"got: {err:#}"
		);
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
