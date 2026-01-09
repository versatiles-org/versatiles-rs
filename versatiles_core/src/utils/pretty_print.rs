//! This module provides utilities for colorized, structured console output.
//! It is mainly used for CLI and testing purposes to display categories, lists,
//! key/value pairs, warnings, and JSON data with indentation and color for better readability.

use crate::json::{JsonValue, stringify_pretty_multi_line};
use colored::{ColoredString, Colorize};
use std::fmt::{Debug, Display};
use std::io::Write;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Low-level writer abstraction that handles output buffering.
/// In runtime, writes directly to stderr; in tests, buffers output in a Vec<u8>.
struct PrettyPrinter {
	indention: String,
	#[cfg(not(any(test, feature = "test")))]
	output: Arc<Mutex<Box<dyn Write + Send>>>,
	#[cfg(any(test, feature = "test"))]
	output: Arc<Mutex<Vec<u8>>>,
}

impl PrettyPrinter {
	pub fn new() -> Self {
		#[cfg(not(any(test, feature = "test")))]
		use std::io::stderr;

		Self {
			indention: String::from("  "),

			#[cfg(not(any(test, feature = "test")))]
			output: Arc::new(Mutex::new(Box::new(stderr()))),

			#[cfg(any(test, feature = "test"))]
			output: Arc::new(Mutex::new(Vec::new())),
		}
	}

	async fn write(&self, text: String) {
		self.output.lock().await.write_all(text.as_bytes()).unwrap();
	}

	#[cfg(any(test, feature = "test"))]
	async fn as_string(&self) -> String {
		use regex::{Regex, RegexBuilder};
		use std::sync::LazyLock;

		static RE_COLORS: LazyLock<Regex> = LazyLock::new(|| RegexBuilder::new("\u{001b}\\[[0-9;]*m").build().unwrap());

		let text: String = String::from_utf8(self.output.lock().await.to_vec()).unwrap();
		RE_COLORS.replace_all(&text, "").to_string()
	}
}

/// High-level interface for structured output with color and indentation.
/// Supports categories, lists, key/value pairs, warnings, and JSON output.
pub struct PrettyPrint {
	prefix: String,
	suffix: String,
	printer: Arc<PrettyPrinter>,
}

impl PrettyPrint {
	#[must_use]
	pub fn new() -> Self {
		Self {
			prefix: String::new(),
			suffix: String::from("\n"),
			printer: Arc::new(PrettyPrinter::new()),
		}
	}

	fn new_indented(&mut self) -> Self {
		Self {
			prefix: format!("{}{}", self.prefix, self.printer.indention),
			suffix: self.suffix.clone(),
			printer: self.printer.clone(),
		}
	}

	/// Writes a bold white category header followed by a colon and returns a new indented PrettyPrint.
	pub async fn get_category(&mut self, text: &str) -> PrettyPrint {
		self.write_line(text.white().bold().to_string() + ":").await;
		self.new_indented()
	}

	/// Writes a white list header followed by a colon and returns a new indented PrettyPrint.
	pub async fn get_list(&mut self, text: &str) -> PrettyPrint {
		self.write_line(text.white().to_string() + ":").await;
		self.new_indented()
	}

	/// Writes a bold yellow warning message.
	pub async fn add_warning(&self, text: &str) {
		self.write_line(text.yellow().bold()).await;
	}

	/// Writes a key and a formatted debug representation of the value.
	pub async fn add_key_value<K: Display + ?Sized, V: Debug + ?Sized>(&self, key: &K, value: &V) {
		self.write_line(format!("{key}: {}", get_formatted_value(value))).await;
	}

	/// Writes a key and a pretty-printed, colorized JSON value.
	pub async fn add_key_json<K: Display + ?Sized>(&self, key: &K, value: &JsonValue) {
		let key_string = format!("{key}: ");
		self
			.write_line(format!(
				"{key_string}{}",
				stringify_pretty_multi_line(value, 80, 1, key_string.len()).bright_green()
			))
			.await;
	}

	/// Writes a formatted debug representation of a value.
	pub async fn add_value<V: Debug>(&self, value: &V) {
		self.write_line(get_formatted_value(value)).await;
	}

	async fn write_line<T: Display>(&self, line: T) {
		self
			.printer
			.write(format!("{}{}{}", self.prefix, line, self.suffix))
			.await;
	}

	#[cfg(any(test, feature = "test"))]
	pub async fn as_string(&self) -> String {
		self.printer.as_string().await
	}
}

impl Default for PrettyPrint {
	fn default() -> Self {
		Self::new()
	}
}

/// Returns a colored string representation of the value based on its type,
/// improving readability by coloring numbers, strings, floats, and custom types differently.
fn get_formatted_value<V: Debug + ?Sized>(value: &V) -> ColoredString {
	let type_name = std::any::type_name::<V>();
	if type_name.starts_with("versatiles_lib::shared::") {
		return format!("{value:?}").bright_blue();
	}
	match type_name {
		"f32" | "f64" => format!("{value:?}").bright_cyan(),
		"i128" | "i16" | "i32" | "i64" | "i8" | "isize" | "u128" | "u16" | "u32" | "u64" | "u8" | "usize" => {
			format_integer(value).bright_cyan()
		}
		"alloc::string::String" | "str" | "&str" => format!("{value:?}").bright_magenta(),
		_ => format!("{value:?}").bright_green(),
	}
}

/// Inserts underscores into large integer strings for better readability.
fn format_integer<V: Debug + ?Sized>(value: &V) -> String {
	let mut text = format!("{value:?}");
	let mut formatted = String::new();
	while (text.len() > 3) && text.chars().nth_back(3).unwrap().is_numeric() {
		let i = text.len() - 3;
		formatted = String::from("_") + &text[i..] + &formatted;
		text = String::from(&text[..i]);
	}
	if formatted.is_empty() { text } else { text + &formatted }
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test_new_pretty_print() {
		let _printer = PrettyPrint::new();
	}

	#[tokio::test]
	async fn test_writers() {
		let mut printer = PrettyPrint::new();

		printer.add_warning("test_warning_1").await;
		let mut cat = printer.get_category("test_category_1").await;
		cat.get_list("test_list_1").await.add_key_value("string_1", &4).await;
		cat.add_warning("test_warning_2").await;
		printer.add_warning("test_warning_3").await;

		let result = printer.as_string().await;
		assert_eq!(
			&result,
			"test_warning_1\ntest_category_1:\n  test_list_1:\n    string_1: 4\n  test_warning_2\ntest_warning_3\n"
		);
	}

	#[test]
	#[should_panic(expected = "assertion")]
	fn x() {
		assert_eq!("Elon Musk", "Genius");
	}
}
