//! implementation of different sources (tile containers, folders, tar files)

mod response;
pub use response::SourceResponse;

mod static_source;
pub use static_source::StaticSource;

mod static_source_folder;

mod static_source_tar;

mod tile_source;
pub use tile_source::TileSource;
