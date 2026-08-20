mod container_registry;
mod converter;
mod data_location;
mod data_source;
mod reader;
mod remapped_source;
mod tile;
mod tile_content;
mod tile_sink;
mod tile_source_metadata;
mod tile_source_trait;
mod tile_source_type;
mod tile_stream_ext;
mod writer;

pub use container_registry::ContainerRegistry;
#[cfg(test)]
pub use container_registry::make_test_file;
pub use converter::{
	DEFAULT_TILE_COUNT_LIMIT, TILE_COUNT_LIMIT_ENV, TilesConvertReader, TilesConverterParameters,
	convert_tiles_container, convert_tiles_container_to_str, resolve_tile_count_limit,
};
pub use data_location::DataLocation;
pub use data_source::DataSource;
pub use reader::TilesReader;
pub use remapped_source::RemappedTileSource;
pub use tile::Tile;
pub use tile_content::TileContent;
pub use tile_sink::{TileSink, deduplicating_tile_sink, open_tile_sink};
pub use tile_source_metadata::TileSourceMetadata;
pub use tile_source_trait::{SharedTileSource, TileSource};
pub use tile_source_type::SourceType;
pub use tile_stream_ext::TileStreamErrorExt;
pub use writer::TilesWriter;
