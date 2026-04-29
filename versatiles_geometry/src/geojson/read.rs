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

/// Returns an iterator over [`GeoFeature`] values parsed from a GeoJSON Text
/// Sequence (RFC 8142): records are separated by the record-separator byte
/// `U+001E` and each record holds one JSON-encoded feature. Unlike NDJSON the
/// JSON inside a record may span multiple physical lines, so we split on the
/// RS byte rather than on newlines.
pub fn read_geojson_seq_iter(mut reader: impl BufRead) -> impl Iterator<Item = Result<GeoFeature>> {
	// Skip everything before the first RS so leading garbage / a stray BOM
	// doesn't confuse the first record.
	let mut prelude = Vec::new();
	let _ = reader.read_until(0x1E, &mut prelude);

	let mut index = 0_usize;
	std::iter::from_fn(move || {
		loop {
			let mut buf = Vec::new();
			match reader.read_until(0x1E, &mut buf) {
				Ok(0) => return None,
				Ok(_) => {
					index += 1;
					// Drop the trailing RS (if any) — it belongs to the next record.
					if buf.last() == Some(&0x1E) {
						buf.pop();
					}
					let text = match std::str::from_utf8(&buf) {
						Ok(s) => s,
						Err(e) => return Some(Err(anyhow!("record {index}: invalid UTF-8: {e}"))),
					};
					// RFC 8142 allows blank records (e.g. trailing whitespace); skip.
					if text.trim().is_empty() {
						continue;
					}
					return Some(
						parse_geojson_feature(&mut ByteIterator::from_reader(Cursor::new(text.to_string()), true))
							.map_err(|e| anyhow!("record {index}: parsing GeoJSON Feature: {e}")),
					);
				}
				Err(e) => return Some(Err(anyhow!("record {}: {e}", index + 1))),
			}
		}
	})
}

/// Returns an iterator over [`GeoFeature`] values from either NDGeoJSON or
/// RFC 8142 GeoJSON Text Sequences. Format is auto-detected from the first
/// non-whitespace byte: a leading `U+001E` selects the sequence parser,
/// anything else selects the line-based parser.
pub fn read_line_delimited_geojson_iter(
	mut reader: impl BufRead + Send + 'static,
) -> Result<Box<dyn Iterator<Item = Result<GeoFeature>> + Send>> {
	let buf = reader.fill_buf()?;
	let leads_with_rs = buf
		.iter()
		.find(|&&b| !b.is_ascii_whitespace())
		.is_some_and(|&b| b == 0x1E);
	if leads_with_rs {
		Ok(Box::new(read_geojson_seq_iter(reader)))
	} else {
		Ok(Box::new(read_ndgeojson_iter(reader)))
	}
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
	use crate::ext::type_name;
	use futures::StreamExt;
	use std::io::{BufReader, Cursor};

	#[test]
	fn test_read_geojson_basic() -> Result<()> {
		let json = r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":{}}]}"#;
		let collection = read_geojson(Cursor::new(json))?;
		assert_eq!(collection.features.len(), 1);
		assert_eq!(type_name(&collection.features[0].geometry), "Point");
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
			assert_eq!(type_name(&feature.geometry), "Point");
		}
	}

	#[test]
	fn read_geojson_seq_iter_with_multiline_records() {
		// RFC 8142: records start with U+001E and the JSON inside a record
		// may span multiple physical lines.
		let single = r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":{}}"#;
		let multi = "{\n  \"type\": \"Feature\",\n  \"geometry\": { \"type\": \"Point\", \"coordinates\": [1, 1] },\n  \"properties\": {}\n}";
		let input = format!("\u{1E}{single}\n\u{1E}{multi}\n");
		let iter = read_geojson_seq_iter(BufReader::new(Cursor::new(input)));
		let results: Vec<_> = iter.collect();
		assert_eq!(results.len(), 2);
		for res in results {
			let feature = res.unwrap();
			assert_eq!(type_name(&feature.geometry), "Point");
		}
	}

	#[test]
	fn read_line_delimited_dispatch_picks_format_from_leading_byte() {
		let nd = "{\"type\":\"Feature\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[0,0]},\"properties\":{}}\n";
		let seq = format!(
			"\u{1E}{}",
			"{\"type\":\"Feature\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[0,0]},\"properties\":{}}\n"
		);
		let iter = read_line_delimited_geojson_iter(BufReader::new(Cursor::new(nd))).unwrap();
		assert_eq!(iter.count(), 1);
		let iter = read_line_delimited_geojson_iter(BufReader::new(Cursor::new(seq))).unwrap();
		assert_eq!(iter.count(), 1);
	}

	#[tokio::test]
	async fn test_read_ndgeojson_stream() {
		let json = r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[2,2]},"properties":{}}"#;
		let input = format!("{json}\n{json}");
		let mut stream = read_ndgeojson_stream(BufReader::new(Cursor::new(input)));
		let mut count = 0;
		while let Some(res) = stream.next().await {
			let feature = res.unwrap();
			assert_eq!(type_name(&feature.geometry), "Point");
			count += 1;
		}
		assert_eq!(count, 2);
	}
}
