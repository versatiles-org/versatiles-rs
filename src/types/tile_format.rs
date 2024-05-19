use std::fmt::Display;

#[cfg(feature = "cli")]
use clap::ValueEnum;
// Enum representing supported tile formats
#[allow(clippy::upper_case_acronyms)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "cli", derive(ValueEnum))]
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

impl Display for TileFormat {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(match self {
			TileFormat::AVIF => "avif",
			TileFormat::BIN => "bin",
			TileFormat::GEOJSON => "geojson",
			TileFormat::JPG => "jpg",
			TileFormat::JSON => "json",
			TileFormat::PBF => "pbf",
			TileFormat::PNG => "png",
			TileFormat::SVG => "svg",
			TileFormat::TOPOJSON => "topojson",
			TileFormat::WEBP => "webp",
		})
	}
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

pub fn extract_format(filename: &mut String) -> Option<TileFormat> {
	if let Some(index) = filename.rfind('.') {
		let format = match filename.get(index..).unwrap() {
			".avif" => TileFormat::AVIF,
			".bin" => TileFormat::BIN,
			".geojson" => TileFormat::GEOJSON,
			".jpg" => TileFormat::JPG,
			".jpeg" => TileFormat::JPG,
			".json" => TileFormat::JSON,
			".pbf" => TileFormat::PBF,
			".png" => TileFormat::PNG,
			".svg" => TileFormat::SVG,
			".topojson" => TileFormat::TOPOJSON,
			".webp" => TileFormat::WEBP,
			_ => return None,
		};
		filename.truncate(index);
		return Some(format);
	}
	None
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_format_to_extension() {
		fn test(format: TileFormat, expected_extension: &str) {
			assert_eq!(
				format_to_extension(&format),
				expected_extension,
				"Extension does not match {expected_extension}"
			);
		}

		test(TileFormat::AVIF, ".avif");
		test(TileFormat::BIN, ".bin");
		test(TileFormat::GEOJSON, ".geojson");
		test(TileFormat::JPG, ".jpg");
		test(TileFormat::JSON, ".json");
		test(TileFormat::PBF, ".pbf");
		test(TileFormat::PNG, ".png");
		test(TileFormat::SVG, ".svg");
		test(TileFormat::TOPOJSON, ".topojson");
		test(TileFormat::WEBP, ".webp");
	}

	#[test]
	fn test_extract_format() {
		fn test(expected_format: Option<TileFormat>, filename: &str, rest: &str) {
			let mut filename_string = String::from(filename);
			assert_eq!(
				extract_format(&mut filename_string),
				expected_format,
				"Extracted compression does not match expected for filename: {filename}"
			);
			assert_eq!(
				filename_string, rest,
				"Filename remainder does not match expected for filename: {filename}"
			);
		}

		test(Some(TileFormat::AVIF), "image.avif", "image");
		test(None, "archive.zip", "archive.zip");
		test(Some(TileFormat::BIN), "binary.bin", "binary");
		test(None, "noextensionfile", "noextensionfile");
		test(None, "unknown.ext", "unknown.ext");
		test(Some(TileFormat::GEOJSON), "data.geojson", "data");
		test(Some(TileFormat::JPG), "image.jpeg", "image");
		test(Some(TileFormat::JPG), "image.jpg", "image");
		test(Some(TileFormat::JSON), "document.json", "document");
		test(Some(TileFormat::PBF), "map.pbf", "map");
		test(Some(TileFormat::PNG), "picture.png", "picture");
		test(Some(TileFormat::SVG), "diagram.svg", "diagram");
		test(Some(TileFormat::SVG), "vector.svg", "vector");
		test(Some(TileFormat::TOPOJSON), "topography.topojson", "topography");
		test(Some(TileFormat::WEBP), "photo.webp", "photo");
	}
}
