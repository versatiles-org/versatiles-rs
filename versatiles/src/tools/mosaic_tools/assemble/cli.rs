use anyhow::{Context, Result, ensure};

#[derive(clap::Args, Debug)]
#[command(
	arg_required_else_help = true,
	disable_version_flag = true,
	about = "Combine many tile containers into a single mosaic",
	long_about = "\
Combine many tile containers into a single mosaic.

Overlays input containers in listed order (earlier wins) and writes one
unified container. Opaque tiles pass through unchanged; tiles with any
transparency are composited in a two-pass pipeline: the first pass streams
every source once and classifies each tile as opaque, empty, or translucent;
the second pass only re-opens the sources needed to composite each
translucent tile.

EXAMPLES

  # Two literal inputs + an output (last path is always the output).
  versatiles mosaic assemble a.versatiles b.versatiles mosaic.versatiles

  # Glob expansion + a list file. Containers listed earlier win.
  versatiles mosaic assemble 'scenes/*.versatiles' extra.txt mosaic.versatiles

  # Cap memory at half of system RAM and drop zooms above 14.
  versatiles mosaic assemble --max-buffer-size 50% --max-zoom 14 \\
      @inputs.lst mosaic.versatiles"
)]
pub struct Assemble {
	#[arg(
		required = true,
		num_args = 2..,
		help = "Input containers followed by the output path (last one wins)",
		long_help = "\
Input containers followed by the output path (last one wins).

Minimum two paths: at least one input and exactly one output. Each
non-output path may be one of:

  * a literal path or URL to a tile container
  * a glob pattern (`*`, `?`, `[...]`) — matches are sorted lexically
  * a `.txt` file — read as a list (one path per line, `#` comments)
  * an `@file` argument — same as `.txt`, regardless of extension

Ordering matters: the first container to supply a given tile wins, so list
higher-priority (e.g. more recent) scenes earlier. The final positional
argument is always treated as the output path."
	)]
	pub(super) paths: Vec<String>,

	/// Lossy WebP quality (0-100) for opaque tiles.
	///
	/// Single value (e.g. "75") applies to every zoom. Comma-separated list
	/// ramps from z=0 upwards ("90,80,70" → z=0 → 90, z=1 → 80, z≥2 → 70).
	/// Use "Z:Q" to jump to zoom Z ("70,14:50,15:20" → z<14 → 70, z=14 → 50,
	/// z≥15 → 20). Translucent tiles ignore this (see --lossless).
	#[arg(long, value_name = "str", default_value = "75")]
	pub(super) quality: String,

	/// Encode translucent tiles losslessly.
	///
	/// By default translucent tiles use the same lossy --quality setting as
	/// opaque tiles. This flag forces lossless WebP for translucent tiles
	/// only — useful when soft edges or semi-transparent overlays must stay
	/// sharp. Opaque tiles remain lossy.
	#[arg(long)]
	pub(super) lossless: bool,

	/// Drop zoom levels below this threshold.
	///
	/// Tiles at z < MIN_ZOOM are excluded from the output. Use to trim
	/// low-resolution overviews if the source contains more zooms than
	/// needed. Default: include every zoom in the inputs.
	#[arg(long, value_name = "int")]
	pub(super) min_zoom: Option<u8>,

	/// Drop zoom levels above this threshold.
	///
	/// Tiles at z > MAX_ZOOM are excluded from the output. Use to cap the
	/// resolution of the mosaic without re-tiling the inputs. Default:
	/// include every zoom in the inputs.
	#[arg(long, value_name = "int")]
	pub(super) max_zoom: Option<u8>,

	#[arg(
		long,
		value_name = "size",
		default_value = "4g",
		help = "Upper bound on the translucent-tile buffer held in memory",
		long_help = "\
Upper bound on the translucent-tile buffer held in memory.

Accepts:

  * plain bytes:    \"4000000000\"
  * SI units:       \"1500m\" = 1.5 GB, \"4g\" = 4 GB, \"0.5t\"
  * RAM percentage: \"50%\" (Linux / macOS only)
  * \"0\":            disables the cap

A bigger buffer means fewer re-scans of the inputs when the translucent
working set is large. If the set doesn't fit, the pipeline splits it into
batches and re-opens the affected sources per batch."
	)]
	pub(super) max_buffer_size: String,
}

pub(super) fn parse_quality(quality: &str) -> Result<[Option<u8>; 32]> {
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
			result[usize::try_from(z).expect("zoom is non-negative")] = Some(quality_val);
		}
	}
	Ok(result)
}

pub(super) fn parse_input_list(content: &str) -> Vec<String> {
	content
		.lines()
		.map(|line| {
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

pub(super) fn resolve_inputs(args: &[String]) -> Result<Vec<String>> {
	let mut result = Vec::new();
	for arg in args {
		if let Some(file_path) = arg.strip_prefix('@') {
			let content = std::fs::read_to_string(file_path)
				.with_context(|| format!("Failed to read input list file: {file_path}"))?;
			result.extend(parse_input_list(&content));
		} else if std::path::Path::new(arg)
			.extension()
			.is_some_and(|ext| ext.eq_ignore_ascii_case("txt"))
		{
			let content =
				std::fs::read_to_string(arg).with_context(|| format!("Failed to read input list file: {arg}"))?;
			result.extend(parse_input_list(&content));
		} else if arg.contains('*') || arg.contains('?') || arg.contains('[') {
			let mut matches: Vec<String> = Vec::new();
			for entry in glob::glob(arg).with_context(|| format!("Invalid glob pattern: {arg}"))? {
				let path = entry.with_context(|| format!("Error reading glob result for: {arg}"))?;
				matches.push(path.to_string_lossy().into_owned());
			}
			ensure!(!matches.is_empty(), "Glob pattern matched no files: {arg}");
			matches.sort();
			result.extend(matches);
		} else {
			result.push(arg.clone());
		}
	}
	Ok(result)
}

pub(super) fn parse_buffer_size(s: &str) -> Result<u64> {
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

		// Percentage (only on platforms where total_system_memory() works)
		#[cfg(any(target_os = "macos", target_os = "linux"))]
		{
			let result = parse_buffer_size("50%").unwrap();
			assert!(result > 0, "50% of system memory should be > 0");
		}
		#[cfg(not(any(target_os = "macos", target_os = "linux")))]
		{
			assert!(parse_buffer_size("50%").is_err());
		}

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

	#[test]
	fn test_resolve_inputs_literal() {
		let args = vec!["a.versatiles".to_string(), "b.versatiles".to_string()];
		let result = resolve_inputs(&args).unwrap();
		assert_eq!(result, vec!["a.versatiles", "b.versatiles"]);
	}

	#[test]
	fn test_resolve_inputs_at_file() {
		let dir = tempfile::tempdir().unwrap();
		let list_path = dir.path().join("inputs.txt");
		std::fs::write(&list_path, "one.versatiles\ntwo.versatiles\n# comment\n").unwrap();

		let args = vec![format!("@{}", list_path.display())];
		let result = resolve_inputs(&args).unwrap();
		assert_eq!(result, vec!["one.versatiles", "two.versatiles"]);
	}

	#[test]
	fn test_resolve_inputs_glob() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("a.versatiles"), "").unwrap();
		std::fs::write(dir.path().join("b.versatiles"), "").unwrap();
		std::fs::write(dir.path().join("c.txt"), "").unwrap();

		let pattern = format!("{}/*.versatiles", dir.path().display());
		let args = vec![pattern];
		let result = resolve_inputs(&args).unwrap();
		assert_eq!(result.len(), 2);
		assert!(result[0].ends_with("a.versatiles"));
		assert!(result[1].ends_with("b.versatiles"));
	}

	#[test]
	fn test_resolve_inputs_txt_file() {
		let dir = tempfile::tempdir().unwrap();
		let list_path = dir.path().join("sources.txt");
		std::fs::write(&list_path, "alpha.versatiles\n# skip\nbeta.versatiles\n").unwrap();

		let args = vec![list_path.to_string_lossy().into_owned()];
		let result = resolve_inputs(&args).unwrap();
		assert_eq!(result, vec!["alpha.versatiles", "beta.versatiles"]);
	}

	#[test]
	fn test_resolve_inputs_glob_no_match() {
		let args = vec!["/nonexistent_dir_xyz/*.versatiles".to_string()];
		assert!(resolve_inputs(&args).is_err());
	}

	#[test]
	fn test_resolve_inputs_at_file_missing() {
		let args = vec!["@/nonexistent/path.txt".to_string()];
		assert!(resolve_inputs(&args).is_err());
	}

	#[test]
	fn test_resolve_inputs_mixed_args() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("a.versatiles"), "").unwrap();
		let list_path = dir.path().join("list.txt");
		std::fs::write(&list_path, "from_list.versatiles\n").unwrap();

		let args = vec![
			"literal.versatiles".to_string(),
			format!("@{}", list_path.display()),
			format!("{}/*.versatiles", dir.path().display()),
		];
		let result = resolve_inputs(&args).unwrap();
		assert_eq!(result.len(), 3);
		assert_eq!(result[0], "literal.versatiles");
		assert_eq!(result[1], "from_list.versatiles");
		assert!(result[2].ends_with("a.versatiles"));
	}

	#[test]
	fn test_parse_quality_single_value_fills_all_zooms() {
		let q = parse_quality("60").unwrap();
		for i in &q {
			assert_eq!(*i, Some(60));
		}
	}

	#[test]
	fn test_parse_quality_zoom_override_cascades() {
		// "90,5:40" → zooms 0=90, 1-4=90 (no override), 5-31=40
		let q = parse_quality("90,5:40").unwrap();
		assert_eq!(q[0], Some(90));
		assert_eq!(q[4], Some(90));
		assert_eq!(q[5], Some(40));
		assert_eq!(q[31], Some(40));
	}

	#[test]
	fn test_parse_quality_boundary_values() {
		assert!(parse_quality("0").is_ok());
		assert!(parse_quality("100").is_ok());
		assert!(parse_quality("101").is_err());
	}

	#[test]
	fn test_parse_quality_invalid_zoom() {
		assert!(parse_quality("32:50").is_err());
	}

	#[test]
	fn test_parse_quality_non_numeric() {
		assert!(parse_quality("abc").is_err());
	}

	#[test]
	fn test_parse_quality_empty_parts() {
		// "80,,70" → zoom 0=80, zoom 1 skipped, zoom 2=70
		let q = parse_quality("80,,70").unwrap();
		assert_eq!(q[0], Some(80));
		assert_eq!(q[2], Some(70));
		assert_eq!(q[31], Some(70));
	}

	#[test]
	fn test_parse_input_list_whitespace_only_lines() {
		let content = "  \n\t\npath.versatiles\n   \n";
		let paths = parse_input_list(content);
		assert_eq!(paths, vec!["path.versatiles"]);
	}

	#[test]
	fn test_parse_input_list_comment_at_start_of_line() {
		let content = "# comment\nfoo.versatiles\n#bar.versatiles";
		let paths = parse_input_list(content);
		assert_eq!(paths, vec!["foo.versatiles"]);
	}

	#[test]
	fn test_parse_buffer_size_zero_with_unit() {
		// "0" is special-cased, but "0g" goes through the normal path
		assert_eq!(parse_buffer_size("0g").unwrap(), 0);
		assert_eq!(parse_buffer_size("0k").unwrap(), 0);
	}

	#[test]
	fn test_parse_buffer_size_boundary_percentage() {
		#[cfg(any(target_os = "macos", target_os = "linux"))]
		{
			assert!(parse_buffer_size("0%").is_ok());
			assert!(parse_buffer_size("100%").is_ok());
		}
		#[cfg(not(any(target_os = "macos", target_os = "linux")))]
		{
			assert!(parse_buffer_size("0%").is_err());
			assert!(parse_buffer_size("100%").is_err());
		}
		assert!(parse_buffer_size("-1%").is_err());
	}

	#[test]
	fn test_resolve_inputs_empty_list_file() {
		let dir = tempfile::tempdir().unwrap();
		let list_path = dir.path().join("empty.txt");
		std::fs::write(&list_path, "# only comments\n\n").unwrap();

		let args = vec![list_path.to_string_lossy().into_owned()];
		let result = resolve_inputs(&args).unwrap();
		assert!(result.is_empty());
	}
}
