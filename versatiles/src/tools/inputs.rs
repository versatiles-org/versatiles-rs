//! Expanding input arguments into a list of data sources: globs, `@file` and
//! `.txt` lists, used by commands that take many inputs.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use versatiles_container::{DataLocation, DataSource};

/// Reads an input list: one data source per line, `#` starts a comment.
pub fn parse_input_list(content: &str) -> Vec<String> {
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

/// Whether `arg` is nothing but a local path, as opposed to a URL, an inline
/// pipeline, JSON, or a path with a `[name,type]` prefix or postfix.
///
/// Only such an argument may be a glob or a list file. `*`, `?` and `[` are
/// also part of the data source syntax: `[osm]a.mbtiles`, `[,vpl](…)` and
/// `https://…/a.pmtiles?key=…` were globs that matched nothing. So this asks
/// [`DataSource::parse`] itself, rather than repeating its grammar.
fn is_plain_path(arg: &str) -> bool {
	// `-` is stdin, and parsing it would read stdin.
	arg != "-"
		&& matches!(
			DataSource::parse(arg).map(DataSource::into_location),
			Ok(DataLocation::Path(path)) if path.as_os_str() == arg
		)
}

/// Expands input arguments into data sources, keeping their order.
///
/// - `@file` is read as an input list, whatever its extension.
/// - A plain path ending in `.txt` is read as an input list.
/// - A plain path containing `*`, `?` or `[` that is not an existing file is a
///   glob; its matches are sorted, and matching nothing is an error.
/// - Anything else, including every data source that isn't a plain path, is
///   passed on unchanged.
pub fn resolve_inputs(args: &[String]) -> Result<Vec<String>> {
	let mut result = Vec::new();
	for arg in args {
		if let Some(file_path) = arg.strip_prefix('@') {
			let content = std::fs::read_to_string(file_path)
				.with_context(|| format!("Failed to read input list file: {file_path}"))?;
			result.extend(parse_input_list(&content));
		} else if !is_plain_path(arg) {
			result.push(arg.clone());
		} else if Path::new(arg)
			.extension()
			.is_some_and(|ext| ext.eq_ignore_ascii_case("txt"))
		{
			let content =
				std::fs::read_to_string(arg).with_context(|| format!("Failed to read input list file: {arg}"))?;
			result.extend(parse_input_list(&content));
		} else if arg.contains(['*', '?', '[']) && !Path::new(arg).exists() {
			let mut matches: Vec<String> = Vec::new();
			for entry in glob::glob(arg).with_context(|| format!("Invalid glob pattern: {arg}"))? {
				let path = entry.with_context(|| format!("Error reading glob result for: {arg}"))?;
				matches.push(path.to_string_lossy().into_owned());
			}
			ensure!(!matches.is_empty(), "glob pattern matched no files: {arg}");
			matches.sort();
			result.extend(matches);
		} else {
			result.push(arg.clone());
		}
	}
	Ok(result)
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
		assert_eq!(paths, Vec::<String>::new());
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
	fn test_resolve_inputs_glob_question_mark_and_class() {
		let dir = tempfile::tempdir().unwrap();
		for name in ["a1.versatiles", "a2.versatiles", "b1.versatiles"] {
			std::fs::write(dir.path().join(name), "").unwrap();
		}

		let args = vec![format!("{}/a?.versatiles", dir.path().display())];
		assert_eq!(resolve_inputs(&args).unwrap().len(), 2);

		let args = vec![format!("{}/[b]1.versatiles", dir.path().display())];
		let result = resolve_inputs(&args).unwrap();
		assert_eq!(result.len(), 1);
		assert!(result[0].ends_with("b1.versatiles"));
	}

	/// A file whose name contains glob characters is that file, not a pattern
	/// that may match others or nothing.
	#[test]
	fn test_resolve_inputs_existing_file_with_glob_characters() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("scene[1].versatiles"), "").unwrap();
		std::fs::write(dir.path().join("scene1.versatiles"), "").unwrap();

		let literal = format!("{}/scene[1].versatiles", dir.path().display());
		let result = resolve_inputs(std::slice::from_ref(&literal)).unwrap();
		assert_eq!(result, vec![literal]);
	}

	/// `*`, `?` and `[` are also part of the data source syntax. Read as globs,
	/// these matched no files and failed.
	#[test]
	fn test_resolve_inputs_passes_data_source_syntax_through() {
		let args: Vec<String> = [
			"[osm]tiles.mbtiles",
			"[osm,mbtiles]tiles/*.db",
			"tiles.db[,mbtiles]",
			"[,vpl](from_container filename=\"a?.versatiles\")",
			"https://example.org/tiles.pmtiles?key=a*b",
			"sftp://host/data/[x].versatiles",
			r#"{"location": "tiles/*.mbtiles"}"#,
			"[osm,vpl]pipeline.txt",
			"https://example.org/list.txt",
		]
		.map(String::from)
		.to_vec();
		assert_eq!(resolve_inputs(&args).unwrap(), args);
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
	fn test_resolve_inputs_empty_list_file() {
		let dir = tempfile::tempdir().unwrap();
		let list_path = dir.path().join("empty.txt");
		std::fs::write(&list_path, "# only comments\n\n").unwrap();

		let args = vec![list_path.to_string_lossy().into_owned()];
		let result = resolve_inputs(&args).unwrap();
		assert_eq!(result, Vec::<String>::new());
	}
}
