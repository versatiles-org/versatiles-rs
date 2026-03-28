use anyhow::{Context, Result, ensure};

/// CLI arguments for `mosaic assemble`.
#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Assemble {
	/// Input container paths, URLs, or glob patterns, followed by the output path.
	/// The last argument is always the output; all preceding arguments are inputs.
	/// Glob patterns (*, ?, [) are expanded. Containers listed earlier overlay
	/// containers listed later. Arguments ending in .txt are read as list files
	/// (one path per line, # comments supported). Use @filename to read any file
	/// as a list regardless of extension.
	#[arg(required = true, num_args = 2..)]
	pub(super) paths: Vec<String>,

	/// Lossy WebP quality for the final output tiles, using zoom-dependent syntax
	/// (e.g. "70,14:50,15:20"). Default: 75.
	#[arg(long, value_name = "str", default_value = "75")]
	pub(super) quality: String,

	/// Encode translucent tiles as lossless WebP instead of using the lossy --quality setting
	#[arg(long)]
	pub(super) lossless: bool,

	/// Minimum zoom level to include in the output (default: include all).
	#[arg(long, value_name = "int")]
	pub(super) min_zoom: Option<u8>,

	/// Maximum zoom level to include in the output (default: include all).
	#[arg(long, value_name = "int")]
	pub(super) max_zoom: Option<u8>,

	/// Maximum memory for the tile buffer (default: 4g).
	/// Supports units: k, m, g, t (e.g. "4g") and % of system memory (e.g. "50%").
	/// Plain number is interpreted as bytes. 0 means unlimited.
	#[arg(long, value_name = "size", default_value = "4g")]
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
			result[usize::try_from(z).unwrap()] = Some(quality_val);
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
}
