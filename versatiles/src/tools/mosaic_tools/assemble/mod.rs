//! Assemble multiple tile containers into a single output by compositing overlapping tiles.
//!
//! # Architecture
//!
//! This module is split into four files:
//!
//! - **`mod.rs`** (this file) — CLI definition, argument parsing, and entry point ([`run`]).
//! - **[`pipeline`]** — The assembly pipeline: opens sources, streams tiles, composites
//!   overlapping images, and writes the output. Contains the main [`pipeline::assemble_tiles`]
//!   loop and all tile-processing functions.
//! - **[`translucent_buffer`]** — Thread-safe buffer holding translucent tiles that are
//!   waiting for further compositing. Keyed by Hilbert index for fast lookup.
//! - **[`pass_state`]** — Multi-pass eviction state. When `--max-buffer-size` is set and the
//!   buffer grows too large, northern tiles are evicted and deferred to subsequent passes.
//!
//! # How assembly works
//!
//! Sources are processed one at a time. Each tile from a source is looked up in the
//! [`TranslucentBuffer`](translucent_buffer::TranslucentBuffer):
//!
//! - **No existing entry + opaque tile** — written directly to the output sink.
//! - **No existing entry + translucent tile** — stored in the buffer for later compositing.
//! - **Existing entry** — the new tile is composited on top of the buffered tile using
//!   additive alpha blending. If the result is opaque, it is written immediately;
//!   otherwise it stays in the buffer.
//!
//! After all sources are processed, remaining buffered tiles are flushed to the output.
//!
//! # Optimizations
//!
//! - **Sweep-line flushing** (`--optimize-order`): Sources are sorted west-to-east. After
//!   each source, tiles in columns that no remaining source can cover are flushed early,
//!   reducing peak memory.
//! - **Multi-pass eviction** (`--max-buffer-size`): When the buffer exceeds the memory
//!   limit, a horizontal cutline (`cut_y`) is computed. Tiles north of the cut are evicted
//!   and the remaining sources only process tiles south of it. After the pass completes, a
//!   new pass processes the evicted (northern) region from scratch. See [`PassState`](pass_state::PassState).

mod pass_state;
mod pipeline;
mod translucent_buffer;

use anyhow::{Context, Result, ensure};
use versatiles_container::TilesRuntime;

/// CLI arguments for `mosaic assemble`.
#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Assemble {
	/// Text file listing container paths or URLs, one per line.
	/// Empty lines and # comments are skipped. Whitespace is trimmed.
	/// Containers listed earlier overlay containers listed later.
	input_list: String,

	/// Output container path (currently supports .tar, directory, or .mbtiles).
	output: String,

	/// Lossy WebP quality for the final output tiles, using zoom-dependent syntax
	/// (e.g. "70,14:50,15:20"). Default: 75.
	#[arg(long, value_name = "str", default_value = "75")]
	quality: String,

	/// Encode translucent tiles as lossless WebP instead of using the lossy --quality setting
	#[arg(long)]
	lossless: bool,

	/// Allow reordering input files to optimize processing speed.
	/// Scans all sources upfront and sorts them west-to-east for efficient sweep-line flushing.
	#[arg(long)]
	optimize_order: bool,

	/// Minimum zoom level to include in the output (default: include all).
	#[arg(long, value_name = "int")]
	min_zoom: Option<u8>,

	/// Maximum zoom level to include in the output (default: include all).
	#[arg(long, value_name = "int")]
	max_zoom: Option<u8>,

	/// Maximum memory for the tile buffer.
	/// Supports units: k, m, g, t (e.g. "4g") and % of system memory (e.g. "50%").
	/// Plain number is interpreted as bytes. 0 means unlimited (default).
	#[arg(long, value_name = "size", default_value = "0")]
	max_buffer_size: String,
}

pub async fn run(args: &Assemble, runtime: &TilesRuntime) -> Result<()> {
	log::info!("mosaic assemble from {:?} to {:?}", args.input_list, args.output);

	let list_content = std::fs::read_to_string(&args.input_list)
		.with_context(|| format!("Failed to read input list file: {}", args.input_list))?;
	let paths = parse_input_list(&list_content);
	ensure!(!paths.is_empty(), "Input list file contains no container paths");

	let quality = parse_quality(&args.quality)?;
	let max_buffer_size = parse_buffer_size(&args.max_buffer_size)?;

	log::info!("assembling {} containers", paths.len());

	let mut pyramids = pipeline::prescan_sources(&paths, runtime).await?;

	// Clip pyramids to requested zoom range once, so downstream code doesn't repeat it.
	for p in &mut pyramids {
		if let Some(min) = args.min_zoom {
			p.set_level_min(min);
		}
		if let Some(max) = args.max_zoom {
			p.set_level_max(max);
		}
	}

	pipeline::assemble_tiles(
		&args.output,
		&paths,
		pyramids,
		&quality,
		args.lossless,
		max_buffer_size,
		args.optimize_order,
		runtime,
	)
	.await
}

/// Parse a quality string (e.g. "70,14:50,15:20") into per-zoom-level values.
fn parse_quality(quality: &str) -> Result<[Option<u8>; 32]> {
	let mut result: [Option<u8>; 32] = [None; 32];
	let mut zoom: i32 = -1;
	for part in quality.split(',') {
		let mut part = part.trim();
		zoom += 1;
		if part.is_empty() {
			continue;
		}
		if let Some(idx) = part.find(':') {
			zoom = part[0..idx].trim().parse()?;
			ensure!((0..=31).contains(&zoom), "Zoom level must be between 0 and 31");
			part = &part[(idx + 1)..];
		}
		let quality_val: u8 = part.trim().parse()?;
		ensure!(quality_val <= 100, "Quality value must be between 0 and 100");
		for z in zoom..32 {
			result[usize::try_from(z).unwrap()] = Some(quality_val);
		}
	}
	Ok(result)
}

/// Parse the input list file, returning a list of container paths/URLs.
fn parse_input_list(content: &str) -> Vec<String> {
	content
		.lines()
		.map(|line| {
			// Strip comments
			let line = if let Some(idx) = line.find('#') {
				&line[..idx]
			} else {
				line
			};
			line.trim().to_string()
		})
		.filter(|line| !line.is_empty())
		.collect()
}

/// Parse a buffer size string into bytes.
///
/// Accepts plain numbers (bytes), numbers with unit suffix (k, m, g, t or kb, mb, gb, tb),
/// or a percentage of total system memory (e.g. "50%").
/// Case-insensitive. Whitespace between number and unit is allowed.
fn parse_buffer_size(s: &str) -> Result<u64> {
	let s = s.trim();
	if s == "0" {
		return Ok(0);
	}

	if let Some(pct) = s.strip_suffix('%') {
		let pct: f64 = pct
			.trim()
			.parse()
			.with_context(|| format!("Invalid percentage in buffer size: {s}"))?;
		ensure!(
			(0.0..=100.0).contains(&pct),
			"Buffer size percentage must be between 0 and 100, got {pct}"
		);
		let total = total_system_memory()?;
		#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
		return Ok((total as f64 * pct / 100.0) as u64);
	}

	let s_lower = s.to_ascii_lowercase();
	// Strip optional trailing "b" (e.g. "kb", "mb", "gb", "tb")
	let s_unit = s_lower.strip_suffix('b').unwrap_or(&s_lower);
	let (num_str, multiplier) = if let Some(n) = s_unit.strip_suffix('t') {
		(n, 1_000_000_000_000u64)
	} else if let Some(n) = s_unit.strip_suffix('g') {
		(n, 1_000_000_000)
	} else if let Some(n) = s_unit.strip_suffix('m') {
		(n, 1_000_000)
	} else if let Some(n) = s_unit.strip_suffix('k') {
		(n, 1_000)
	} else {
		(s_lower.as_str(), 1)
	};

	let num: f64 = num_str
		.trim()
		.parse()
		.with_context(|| format!("Invalid buffer size: {s}"))?;
	ensure!(num >= 0.0, "Buffer size must not be negative: {s}");
	#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
	Ok((num * multiplier as f64) as u64)
}

/// Return total physical memory in bytes.
fn total_system_memory() -> Result<u64> {
	#[cfg(target_os = "macos")]
	{
		let output = std::process::Command::new("sysctl")
			.args(["-n", "hw.memsize"])
			.output()
			.context("failed to run sysctl")?;
		ensure!(output.status.success(), "sysctl hw.memsize failed");
		let s = String::from_utf8_lossy(&output.stdout);
		s.trim().parse::<u64>().context("failed to parse hw.memsize")
	}
	#[cfg(target_os = "linux")]
	{
		let content = std::fs::read_to_string("/proc/meminfo").context("failed to read /proc/meminfo")?;
		for line in content.lines() {
			if let Some(rest) = line.strip_prefix("MemTotal:") {
				let kb_str = rest.trim().trim_end_matches("kB").trim();
				let kb: u64 = kb_str.parse().context("failed to parse MemTotal")?;
				return Ok(kb * 1024);
			}
		}
		anyhow::bail!("MemTotal not found in /proc/meminfo")
	}
	#[cfg(not(any(target_os = "macos", target_os = "linux")))]
	{
		anyhow::bail!("Cannot detect system memory on this platform; use an absolute size instead of %")
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_input_list() {
		let content = "
# This is a comment
tiles/001.versatiles
tiles/002.versatiles

  tiles/003.versatiles
# Another comment
https://example.com/tiles/004.versatiles
";
		let paths = parse_input_list(content);
		assert_eq!(
			paths,
			vec![
				"tiles/001.versatiles",
				"tiles/002.versatiles",
				"tiles/003.versatiles",
				"https://example.com/tiles/004.versatiles",
			]
		);
	}

	#[test]
	fn test_parse_input_list_inline_comments() {
		let content = "tiles/001.versatiles # first container\ntiles/002.versatiles";
		let paths = parse_input_list(content);
		assert_eq!(paths, vec!["tiles/001.versatiles", "tiles/002.versatiles"]);
	}

	#[test]
	fn test_parse_input_list_empty() {
		let content = "\n# only comments\n  \n";
		let paths = parse_input_list(content);
		assert!(paths.is_empty());
	}

	#[test]
	fn test_parse_buffer_size() {
		// Plain bytes
		assert_eq!(parse_buffer_size("0").unwrap(), 0);
		assert_eq!(parse_buffer_size("1024").unwrap(), 1024);

		// Units (case-insensitive)
		assert_eq!(parse_buffer_size("1k").unwrap(), 1_000);
		assert_eq!(parse_buffer_size("1K").unwrap(), 1_000);
		assert_eq!(parse_buffer_size("1Kb").unwrap(), 1_000);
		assert_eq!(parse_buffer_size("1t").unwrap(), 1_000_000_000_000);
		assert_eq!(parse_buffer_size("2m").unwrap(), 2_000_000);
		assert_eq!(parse_buffer_size("2M").unwrap(), 2_000_000);
		assert_eq!(parse_buffer_size("2mB").unwrap(), 2_000_000);
		assert_eq!(parse_buffer_size("3g").unwrap(), 3_000_000_000);
		assert_eq!(parse_buffer_size("3G").unwrap(), 3_000_000_000);
		assert_eq!(parse_buffer_size("3gb").unwrap(), 3_000_000_000);

		// Fractional with unit
		assert_eq!(parse_buffer_size("1.5g").unwrap(), 1_500_000_000);
		assert_eq!(parse_buffer_size("0.5m").unwrap(), 500_000);

		// Whitespace
		assert_eq!(parse_buffer_size("  4g  ").unwrap(), 4_000_000_000);
		assert_eq!(parse_buffer_size("2 m").unwrap(), 2_000_000);

		// Percentage
		let result = parse_buffer_size("50%").unwrap();
		assert!(result > 0, "50% of system memory should be > 0");

		// Errors
		assert!(parse_buffer_size("abc").is_err());
		assert!(parse_buffer_size("-1g").is_err());
		assert!(parse_buffer_size("101%").is_err());
	}

	#[test]
	fn test_parse_quality() {
		let q = parse_quality("80").unwrap();
		assert_eq!(q[0], Some(80));
		assert_eq!(q[15], Some(80));

		let q = parse_quality("80,70,14:50,15:20").unwrap();
		assert_eq!(q[0], Some(80));
		assert_eq!(q[1], Some(70));
		assert_eq!(q[13], Some(70));
		assert_eq!(q[14], Some(50));
		assert_eq!(q[15], Some(20));
	}
}
