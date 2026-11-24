//! Path patterns for the `immutable` list of a static source (#337).
//!
//! A pattern is matched against a file's path inside its source, segment by
//! segment: `*` matches any part of one segment, `**` as a whole segment
//! matches any number of segments, none included. Nothing else is special.

use std::fmt;

use anyhow::{Result, bail};

/// One segment of a [`PathPattern`].
#[derive(Debug, Clone, PartialEq)]
enum Segment {
	/// `**`: any number of segments, none included.
	AnySegments,
	/// A segment that may contain `*`, kept as the parts between them.
	Parts(Vec<String>),
}

/// A pattern such as `assets/lib/*@*/**`, matched against paths inside a
/// static source.
#[derive(Debug, Clone, PartialEq)]
pub struct PathPattern {
	text: String,
	segments: Vec<Segment>,
}

impl PathPattern {
	/// Parses `text`, refusing what would never match the way it reads.
	///
	/// # Errors
	/// Returns an error for an empty pattern, a leading `/` (patterns are paths
	/// inside the source, without its `prefix`), an empty segment, a `.` or
	/// `..` segment, or a `**` that is not a whole segment.
	pub fn parse(text: &str) -> Result<PathPattern> {
		if text.is_empty() {
			bail!("immutable pattern is empty");
		}
		if text.starts_with('/') {
			bail!(
				"immutable pattern {text:?} starts with '/': patterns are paths inside the static source, without its prefix, e.g. {:?}",
				text.trim_start_matches('/')
			);
		}
		let segments = text
			.split('/')
			.map(|segment| match segment {
				"" => bail!("immutable pattern {text:?} has an empty segment"),
				"." | ".." => bail!("immutable pattern {text:?} has a {segment:?} segment"),
				"**" => Ok(Segment::AnySegments),
				_ if segment.contains("**") => {
					bail!("immutable pattern {text:?}: '**' must be a whole segment, as in \"a/**/b\"")
				}
				_ => Ok(Segment::Parts(segment.split('*').map(str::to_string).collect())),
			})
			.collect::<Result<Vec<_>>>()?;
		Ok(PathPattern {
			text: text.to_string(),
			segments,
		})
	}

	/// Whether the path made of `segments` matches.
	#[must_use]
	pub fn matches<S: AsRef<str>>(&self, segments: &[S]) -> bool {
		matches_from(&self.segments, segments)
	}
}

impl fmt::Display for PathPattern {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.text)
	}
}

fn matches_from<S: AsRef<str>>(pattern: &[Segment], path: &[S]) -> bool {
	match pattern.split_first() {
		None => path.is_empty(),
		Some((Segment::AnySegments, rest)) => (0..=path.len()).any(|skip| matches_from(rest, &path[skip..])),
		Some((Segment::Parts(parts), rest)) => match path.split_first() {
			Some((segment, path_rest)) => segment_matches(parts, segment.as_ref()) && matches_from(rest, path_rest),
			None => false,
		},
	}
}

/// Whether `segment` matches the parts of a pattern segment, which stand
/// between its `*`s: the first part is a prefix, the last a suffix, and the
/// others appear in order between them.
fn segment_matches(parts: &[String], segment: &str) -> bool {
	let (first, rest) = parts.split_first().expect("split always yields at least one part");
	let Some(mut remaining) = segment.strip_prefix(first.as_str()) else {
		return false;
	};
	let Some((last, middle)) = rest.split_last() else {
		// No `*` at all: the whole segment must match.
		return remaining.is_empty();
	};
	for part in middle {
		match remaining.find(part.as_str()) {
			Some(index) => remaining = &remaining[index + part.len()..],
			None => return false,
		}
	}
	remaining.len() >= last.len() && remaining.ends_with(last.as_str())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn matches(pattern: &str, path: &str) -> bool {
		let segments: Vec<&str> = path.split('/').collect();
		PathPattern::parse(pattern).unwrap().matches(&segments)
	}

	#[test]
	fn versioned_folders_match_at_any_depth() {
		let pattern = "assets/lib/*@*/**";
		assert!(matches(pattern, "assets/lib/maplibre-gl@5.6.0/maplibre-gl.js"));
		assert!(matches(pattern, "assets/lib/maplibre-gl@5.6.0/dist/sub/file.css"));
		assert!(!matches(pattern, "assets/lib/maplibre-gl/maplibre-gl.js"));
		assert!(
			matches(pattern, "assets/lib/maplibre-gl@5.6.0"),
			"`**` also matches no segment"
		);
		assert!(!matches(pattern, "other/lib/maplibre-gl@5.6.0/maplibre-gl.js"));
	}

	#[test]
	fn star_stays_within_a_segment() {
		assert!(matches("*.js", "app.js"));
		assert!(!matches("*.js", "lib/app.js"));
		assert!(matches("a*b*c", "abc"));
		assert!(matches("a*b*c", "a-b-b-c"));
		assert!(!matches("a*b*c", "a-c"));
		assert!(!matches("ab*ba", "aba"), "prefix and suffix must not overlap");
		assert!(matches("*", "anything"));
		assert!(!matches("exact", "exactly"));
	}

	#[test]
	fn double_star_matches_none_or_many_segments() {
		assert!(matches("**/x.js", "x.js"));
		assert!(matches("**/x.js", "a/b/x.js"));
		assert!(matches("a/**/z", "a/z"));
		assert!(matches("a/**/z", "a/b/c/z"));
		assert!(!matches("a/**/z", "a/b/c"));
		assert!(matches("**", "any/path/at/all"));
	}

	#[test]
	fn misleading_patterns_are_refused() {
		for (pattern, reason) in [
			("", "is empty"),
			("/assets/lib/*@*/**", "starts with '/'"),
			("assets//lib", "empty segment"),
			("assets/../lib", "\"..\" segment"),
			("assets/lib**", "whole segment"),
		] {
			let error = PathPattern::parse(pattern).unwrap_err().to_string();
			assert!(error.contains(reason), "{pattern:?}: {error}");
		}
	}

	#[test]
	fn a_leading_slash_names_the_fix() {
		let error = PathPattern::parse("/assets/**").unwrap_err().to_string();
		assert!(error.contains("\"assets/**\""), "{error}");
	}
}
