use crate::utils::geometry::{GeoProperties, GeoValue};
use anyhow::{Context, Result};
use std::{io::Read, path::Path};

pub fn read_csv_file(path: &Path) -> Result<Vec<GeoProperties>> {
	std::fs::File::open(path)
		.with_context(|| format!("Failed to open file at path: {:?}", path))
		.and_then(|file| parse_csv(&mut std::io::BufReader::new(file)))
}

pub fn parse_csv(reader: &mut dyn Read) -> Result<Vec<GeoProperties>> {
	let mut data: Vec<GeoProperties> = Vec::new();

	let mut csv_reader = csv::Reader::from_reader(reader);
	let header: Vec<(usize, String)> = csv_reader
		.headers()
		.context("Failed to read CSV headers")?
		.iter()
		.map(|s| s.to_string())
		.enumerate()
		.collect();

	for record in csv_reader.records() {
		let record = record.context("Failed to read CSV record")?;
		if record.len() < header.len() {
			continue;
		}
		let mut entry = GeoProperties::new();
		for (col_index, name) in header.iter() {
			entry.insert(
				name.to_string(),
				GeoValue::parse_str(
					record
						.get(*col_index)
						.with_context(|| format!("Failed to get value for column: {}", name))?,
				),
			);
		}
		data.push(entry);
	}

	Ok(data)
}
