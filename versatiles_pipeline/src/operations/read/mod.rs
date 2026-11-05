pub mod from_color;
pub mod from_container;
pub mod from_csv;
pub mod from_debug;
#[cfg(feature = "gdal")]
pub mod from_gdal;
pub mod from_geo;
pub mod from_grid;
pub mod from_h3;
pub mod from_merged_vector;
pub mod from_stacked;
pub mod from_stacked_raster;
pub mod from_tile;
pub mod from_tilejson;

mod gathering;

/// Don't bother showing a progress bar for tiny inputs — the bar would
/// flicker once and disappear. 10 MB is the smallest size where users start
/// to notice the wait.
const PROGRESS_MIN_BYTES: u64 = 10_000_000;
