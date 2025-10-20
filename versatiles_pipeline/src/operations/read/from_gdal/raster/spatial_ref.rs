use anyhow::Result;
use gdal::spatial_ref::{AxisMappingStrategy, SpatialRef};

pub fn get_spatial_ref(epsg: u32) -> Result<SpatialRef> {
	let mut srs = SpatialRef::from_epsg(epsg).map_err(|e| anyhow::anyhow!("Failed to get spatial reference: {}", e))?;
	srs.set_axis_mapping_strategy(AxisMappingStrategy::TraditionalGisOrder);
	Ok(srs)
}
