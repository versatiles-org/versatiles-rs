//! This module provides functions for reading GeoJSON and newline-delimited GeoJSON (NDJSON/NDGeoJSON) data into internal geometry types.
//! It supports full-file parsing, line-by-line iteration, and asynchronous streaming via `futures`.
//! It integrates with the crate’s custom `ByteIterator` and the `#[context]` macro for detailed error handling.

use super::parse_geojson;
use crate::{
	geo::{GeoCollection, GeoFeature},
	geojson::parse_geojson_feature,
};
use anyhow::{Error, Result, anyhow};
use futures::{Stream, StreamExt, future::ready, stream};
use std::io::{BufRead, Cursor, Read};
use versatiles_core::byte_iterator::ByteIterator;
use versatiles_derive::context;

/// Reads an entire GeoJSON document from any `Read` source and parses it into a [`GeoCollection`].
///
/// This function loads the full input into memory and uses [`parse_geojson`] for parsing.
/// Returns an error if the input cannot be read or parsed.
#[context("reading full GeoJSON document")]
pub fn read_geojson(mut reader: impl Read) -> Result<GeoCollection> {
	let mut buffer = String::new();
	reader.read_to_string(&mut buffer)?;
	parse_geojson(&buffer)
}

/// Internal helper that processes a single NDGeoJSON line.
///
/// Skips empty lines, parses valid GeoJSON features, and wraps errors with line number context.
#[context("processing GeoJSON line {}", index + 1)]
fn process_line(line: std::io::Result<String>, index: usize) -> Result<Option<GeoFeature>> {
	match line {
		Ok(line) if line.trim().is_empty() => Ok(None), // Skip empty or whitespace-only lines
		Ok(line) => parse_geojson_feature(&mut ByteIterator::from_reader(Cursor::new(line), true))
			.map(Some)
			.map_err(|e| anyhow!("line {}: {}", index + 1, e)),
		Err(e) => Err(anyhow!("line {}: {}", index + 1, e)),
	}
}

/// Returns an iterator over [`GeoFeature`] values parsed from newline-delimited GeoJSON (NDGeoJSON).
///
/// Each line of input is expected to be a valid GeoJSON feature.
/// Empty lines are ignored, and parsing errors include the offending line number.
pub fn read_ndgeojson_iter(reader: impl BufRead) -> impl Iterator<Item = Result<GeoFeature>> {
	reader
		.lines()
		.enumerate()
		.filter_map(|(index, line)| process_line(line, index).transpose())
}

/// Returns an asynchronous stream of [`GeoFeature`] values parsed from NDGeoJSON input.
///
/// Uses `tokio::spawn` to parallelize parsing across CPU cores.
/// Each item in the stream is a [`Result<GeoFeature>`], and errors contain contextual information.
pub fn read_ndgeojson_stream(reader: impl BufRead) -> impl Stream<Item = Result<GeoFeature>> {
	stream::iter(reader.lines().enumerate())
		.map(|(index, line)| tokio::spawn(async move { process_line(line, index).transpose() }))
		.buffered(num_cpus::get())
		.filter_map(|f| {
			ready(match f {
				Ok(value) => value,
				Err(e) => Some(Err(Error::from(e))),
			})
		})
}

#[cfg(test)]
mod tests {
	use super::*;
	use futures::StreamExt;
	use std::io::{BufReader, Cursor};

	#[test]
	fn test_read_geojson_basic() -> Result<()> {
		let json = r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":{}}]}"#;
		let collection = read_geojson(Cursor::new(json))?;
		assert_eq!(collection.features.len(), 1);
		assert_eq!(collection.features[0].geometry.type_name(), "Point");
		Ok(())
	}

	#[test]
	fn test_read_ndgeojson_iter_with_empty_lines() {
		let json = r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[1,1]},"properties":{}}"#;
		let input = format!("{json}\n\n{json}");
		let iter = read_ndgeojson_iter(BufReader::new(Cursor::new(input)));
		let results: Vec<_> = iter.collect();
		assert_eq!(results.len(), 2);
		for res in results {
			let feature = res.unwrap();
			assert_eq!(feature.geometry.type_name(), "Point");
		}
	}

	#[tokio::test]
	async fn test_read_ndgeojson_stream() {
		let json = r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[2,2]},"properties":{}}"#;
		let input = format!("{json}\n{json}");
		let mut stream = read_ndgeojson_stream(BufReader::new(Cursor::new(input)));
		let mut count = 0;
		while let Some(res) = stream.next().await {
			let feature = res.unwrap();
			assert_eq!(feature.geometry.type_name(), "Point");
			count += 1;
		}
		assert_eq!(count, 2);
	}
}
