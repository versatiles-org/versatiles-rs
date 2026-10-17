//! Reading tile coordinates out of path components, for the containers that
//! store tiles as files: a directory tree and a tar archive.

/// Parses one component of a `z/x/y` tile path, or `None` when it is not one.
///
/// Digits only, and no leading zero — the rule the tile server applies to
/// URLs. `parse` alone takes `+3`, and `01` and `1` would both become tile 1,
/// two files for one tile with whichever comes last silently winning.
pub(crate) fn parse_tile_component<T: std::str::FromStr>(text: &str) -> Option<T> {
	if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
		return None;
	}
	if text.len() > 1 && text.starts_with('0') {
		return None;
	}
	text.parse().ok()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn only_plain_decimal_numbers_are_tile_components() {
		assert_eq!(parse_tile_component::<u32>("0"), Some(0));
		assert_eq!(parse_tile_component::<u32>("17"), Some(17));
		for text in ["", "01", "+1", "-1", "1a", " 1", "1.5"] {
			assert_eq!(parse_tile_component::<u32>(text), None, "{text:?}");
		}
		assert_eq!(parse_tile_component::<u8>("256"), None);
	}
}
