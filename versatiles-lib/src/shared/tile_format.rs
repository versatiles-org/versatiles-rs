#[cfg(feature = "full")]
use clap::ValueEnum;
// Enum representing supported tile formats
#[allow(clippy::upper_case_acronyms)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "full", derive(ValueEnum))]
pub enum TileFormat {
	AVIF,
	BIN,
	GEOJSON,
	JPG,
	JSON,
	PBF,
	PNG,
	SVG,
	TOPOJSON,
	WEBP,
}

pub fn format_to_extension(format: &TileFormat) -> String {
	String::from(match format {
		TileFormat::AVIF => ".avif",
		TileFormat::BIN => ".bin",
		TileFormat::GEOJSON => ".geojson",
		TileFormat::JPG => ".jpg",
		TileFormat::JSON => ".json",
		TileFormat::PBF => ".pbf",
		TileFormat::PNG => ".png",
		TileFormat::SVG => ".svg",
		TileFormat::TOPOJSON => ".topojson",
		TileFormat::WEBP => ".webp",
	})
}
