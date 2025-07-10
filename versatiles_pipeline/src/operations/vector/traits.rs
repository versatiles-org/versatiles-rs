use anyhow::Result;
use versatiles_core::tilejson::TileJSON;
use versatiles_geometry::vector_tile::VectorTile;

pub trait RunnerTrait {
	fn update_tilejson(&self, tilejson: &mut TileJSON);
	fn run(&self, tile: VectorTile) -> Result<VectorTile>;
}
