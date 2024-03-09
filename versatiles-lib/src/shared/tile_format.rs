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

pub fn extract_format(filename: &mut String) -> TileFormat {
	if let Some(index) = filename.rfind(".") {
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
			_ => TileFormat::BIN,
		};
		filename.truncate(index);
		return format;
	}
	TileFormat::BIN
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
		fn test(expected_format: TileFormat, filename: &str, rest: &str) {
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

		test(TileFormat::AVIF, "image.avif", "image");
		test(TileFormat::BIN, "archive.zip", "archive");
		test(TileFormat::BIN, "binary.bin", "binary");
		test(TileFormat::BIN, "binary", "binary");
		test(TileFormat::BIN, "noextensionfile", "noextensionfile");
		test(TileFormat::BIN, "unknown.ext", "unknown");
		test(TileFormat::GEOJSON, "data.geojson", "data");
		test(TileFormat::JPG, "image.jpeg", "image");
		test(TileFormat::JPG, "image.jpg", "image");
		test(TileFormat::JSON, "document.json", "document");
		test(TileFormat::PBF, "map.pbf", "map");
		test(TileFormat::PNG, "picture.png", "picture");
		test(TileFormat::SVG, "diagram.svg", "diagram");
		test(TileFormat::SVG, "vector.svg", "vector");
		test(TileFormat::TOPOJSON, "topography.topojson", "topography");
		test(TileFormat::WEBP, "photo.webp", "photo");
	}
}
