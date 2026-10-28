//! implementation of different sources (tile containers, folders, tar files)

mod response;
mod static_source;
mod static_source_folder;
mod static_source_remote_folder;
mod static_source_tar;
mod tile_source;

pub use response::{SourceResponse, TileResponse};
pub use static_source::StaticSource;
pub use tile_source::ServerTileSource;
#[cfg(fuzzing)]
pub(super) use {
	static_source_remote_folder::RemoteFolder, static_source_tar::resolve_path, tile_source::parse_tile_coord,
};
