use super::parse_geojson;
use crate::{GeoCollection, GeoFeature, parse_geojson_feature};
use anyhow::{Error, Result, anyhow};
use futures::{Stream, StreamExt, future::ready, stream};
use std::io::{BufRead, Cursor, Read};
use versatiles_core::byte_iterator::ByteIterator;

pub fn read_geojson(mut reader: impl Read) -> Result<GeoCollection> {
	let mut buffer = String::new();
	reader.read_to_string(&mut buffer)?;
	parse_geojson(&buffer)
}

fn process_line(line: std::io::Result<String>, index: usize) -> Result<Option<GeoFeature>> {
	match line {
		Ok(line) if line.trim().is_empty() => Ok(None), // Skip empty or whitespace-only lines
		Ok(line) => parse_geojson_feature(&mut ByteIterator::from_reader(Cursor::new(line), true))
			.map(Some)
			.map_err(|e| anyhow!("line {}: {}", index + 1, e)),
		Err(e) => Err(anyhow!("line {}: {}", index + 1, e)),
	}
}

pub fn read_ndgeojson_iter(reader: impl BufRead) -> impl Iterator<Item = Result<GeoFeature>> {
	reader
		.lines()
		.enumerate()
		.filter_map(|(index, line)| process_line(line, index).transpose())
}

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
