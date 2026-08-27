//! Separator parameters: one character, written directly or named by an escape.
//!
//! `from_csv`'s `delimiter` and `vector_update_properties`' `field_separator`
//! and `decimal_separator` are the same parameter with one extra constraint on
//! the first, so they share one spelling rule here rather than each carrying
//! its own. They did not, once: `field_separator='\t'` meant a tab while
//! `delimiter='\t'` was an error, with a message that printed the value back
//! and appeared to deny what the reader had just written.

use anyhow::{Result, anyhow, bail};

/// Decodes the character a separator parameter names.
///
/// Accepts the character itself — `;`, or a real tab, which is what VPL's own
/// `"\t"` resolves to — and the two-character forms `\t`, `\n` and `\r`, which
/// is what `'\t'` and `"\\t"` arrive as. Both spellings are accepted because
/// VPL resolves escapes only inside double quotes, and its parser rejects `\r`
/// outright: the two-character form is the only way to write a carriage return
/// at all, so it cannot simply be dropped in favour of one rule.
///
/// # Errors
///
/// When the value names neither. `both_types_accept_the_same_spellings` below
/// is the list of what it takes; `helpers` is a private module, so that cannot
/// be a doctest.
pub fn decode_separator(value: &str) -> Result<char> {
	match value {
		"\\t" | "\t" => Ok('\t'),
		"\\n" | "\n" => Ok('\n'),
		"\\r" | "\r" => Ok('\r'),
		// `str::len` is bytes, so this is ASCII-only: a multi-byte character is
		// two bytes and falls to the error arm. Preserved rather than widened —
		// both readers below want a byte, and nothing has asked for `é`.
		_ if value.len() == 1 => Ok(value.chars().next().expect("a one-byte string has a character")),
		_ => bail!("separator must be one character or an escape (\\t, \\n, \\r), got {value:?}"),
	}
}

/// A CSV separator, as one character.
///
/// Carries the format of `field_separator=` and `decimal_separator=` in their
/// type, so a two-character separator is refused where the value is decoded
/// rather than when the file is read (#257).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeparatorChar(char);

impl SeparatorChar {
	/// The separator as a single character.
	#[must_use]
	pub fn char(self) -> char {
		self.0
	}
}

impl TryFrom<&str> for SeparatorChar {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self> {
		decode_separator(value).map(Self)
	}
}

/// A CSV field delimiter: the same spellings as [`SeparatorChar`], narrowed to
/// the single ASCII byte the reader takes.
///
/// A byte rather than a `char` because that is what `csv::ReaderBuilder` wants;
/// a multi-byte delimiter is not something it could use. The narrowing is the
/// only difference between the two types, and stating it here is what keeps the
/// two parameters spelled the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CsvDelimiter(u8);

impl CsvDelimiter {
	/// The delimiter as the single byte the reader takes.
	#[must_use]
	pub fn byte(self) -> u8 {
		self.0
	}
}

impl TryFrom<&str> for CsvDelimiter {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self> {
		let character = decode_separator(value)?;
		// Unreachable while `decode_separator` is ASCII-only, and deliberately
		// still here: if it ever widens, this is the difference between a
		// refusal and a silently truncated delimiter.
		let byte = u8::try_from(character).map_err(|_| anyhow!("delimiter must be ASCII, got {character:?}"))?;
		Ok(Self(byte))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The bug this module exists to remove: every spelling means the same
	/// thing to both parameters. `field_separator='\t'` used to be a tab while
	/// `delimiter='\t'` was an error.
	#[test]
	fn both_types_accept_the_same_spellings() {
		for (value, expected) in [
			(";", ';'),
			("\t", '\t'),  // what VPL's `"\t"` resolves to
			("\\t", '\t'), // what `'\t'` and `"\\t"` arrive as
			("\\n", '\n'),
			("\\r", '\r'), // the only way to write a carriage return
			("\n", '\n'),
			("\r", '\r'),
		] {
			assert_eq!(SeparatorChar::try_from(value).unwrap().char(), expected, "{value:?}");
			assert_eq!(
				CsvDelimiter::try_from(value).unwrap().byte(),
				expected as u8,
				"{value:?}"
			);
		}
	}

	#[test]
	fn both_types_refuse_the_same_values() {
		for value in [";;", "", "abc", "\\q", "é"] {
			assert!(SeparatorChar::try_from(value).is_err(), "{value:?}");
			assert!(CsvDelimiter::try_from(value).is_err(), "{value:?}");
		}
	}

	/// The message used to print the value raw, so a literal backslash-t read
	/// back as the escape the author thought they had written — a refusal that
	/// looked like it was denying its own rule.
	#[test]
	fn the_message_distinguishes_an_escape_from_the_character() {
		let two_chars = decode_separator("\\q").unwrap_err().to_string();
		assert!(two_chars.contains(r#""\\q""#), "{two_chars}");

		let with_a_tab = decode_separator("a\tb").unwrap_err().to_string();
		assert!(with_a_tab.contains(r#""a\tb""#), "{with_a_tab}");
	}
}
