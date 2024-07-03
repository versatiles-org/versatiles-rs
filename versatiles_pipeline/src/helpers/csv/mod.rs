mod csv_lines;
mod csv_parser;

use crate::{
	geometry::{GeoProperties, GeoValue},
	utils::progress::get_progress_bar,
};
use anyhow::{Context, Result};
use csv_parser::CsvParser;
use std::{os::unix::fs::MetadataExt, path::Path};

/// Reads a CSV file from the given path and returns a vector of `GeoProperties`.
///
/// # Arguments
///
/// * `path` - A reference to the path of the CSV file.
///
/// # Returns
///
/// * `Result<Vec<GeoProperties>>` - A vector of `GeoProperties` or an error if the file could not be read.
pub fn read_csv_file(path: &Path) -> Result<Vec<GeoProperties>> {
	let file = std::fs::File::open(path)
		.with_context(|| format!("Failed to open file at path: {:?}", path))?;

	let size = path.metadata()?.size();

	let mut data: Vec<GeoProperties> = Vec::new();

	let mut csv_reader = CsvParser::new(file, ',');
	let mut lines = csv_reader.lines();
	let header: Vec<(usize, String)> = lines
		.next()
		.unwrap()
		.0?
		.into_iter()
		.enumerate()
		.collect::<Vec<_>>();

	let mut progress = get_progress_bar("read csv", size);
	for line in lines {
		progress.set_position(line.1 as u64);

		let record = line.0?;
		let mut entry = GeoProperties::new();
		for (col_index, name) in header.iter() {
			entry.insert(
				name.to_string(),
				GeoValue::parse_str(
					record
						.get(*col_index)
						.with_context(|| format!("Failed to get value for column: {name}"))?,
				),
			);
		}
		data.push(entry);
	}
	progress.finish();

	Ok(data)
}
