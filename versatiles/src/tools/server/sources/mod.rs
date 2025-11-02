//! implementation of different sources (tile containers, folders, tar files)

mod response;
mod static_source;
mod static_source_folder;
mod static_source_tar;
mod tile_source;

pub use response::SourceResponse;
pub use static_source::StaticSource;
pub use tile_source::TileSource;
