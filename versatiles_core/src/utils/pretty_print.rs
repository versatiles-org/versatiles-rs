use colored::*;
use std::fmt::{Debug, Display};
use std::io::Write;
use std::sync::Arc;
use tokio::sync::Mutex;

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
			indention: String::from("   "),

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
		use lazy_static::lazy_static;
		use regex::{Regex, RegexBuilder};

		lazy_static! {
			static ref RE_COLORS: Regex = RegexBuilder::new("\u{001b}\\[[0-9;]*m").build().unwrap();
		}

		let text: String = String::from_utf8(self.output.lock().await.to_vec()).unwrap();
		RE_COLORS.replace_all(&text, "").to_string()
	}
}

pub struct PrettyPrint {
	prefix: String,
	suffix: String,
	printer: Arc<PrettyPrinter>,
}

impl PrettyPrint {
	pub fn new() -> Self {
		Self {
			prefix: String::from(""),
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

	pub async fn get_category(&mut self, text: &str) -> PrettyPrint {
		self.write_line(text.white().bold().to_string() + ":").await;
		self.new_indented()
	}

	pub async fn get_list(&mut self, text: &str) -> PrettyPrint {
		self.write_line(text.white().to_string() + ":").await;
		self.new_indented()
	}

	pub async fn add_warning(&self, text: &str) {
		self.write_line(text.yellow().bold()).await;
	}

	pub async fn add_key_value<K: Display + ?Sized, V: Debug + ?Sized>(&self, key: &K, value: &V) {
		self
			.write_line(format!("{key}: {}", get_formatted_value(value)))
			.await;
	}

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

	#[allow(dead_code)]
	pub async fn add_str(&self, text: String) {
		self.printer.write(text).await;
	}
}

impl Default for PrettyPrint {
	fn default() -> Self {
		Self::new()
	}
}

fn get_formatted_value<V: Debug + ?Sized>(value: &V) -> ColoredString {
	let type_name = std::any::type_name::<V>();
	if type_name.starts_with("versatiles_lib::shared::") {
		return format!("{:?}", value).bright_blue();
	}
	match type_name {
		"f32" | "f64" => format!("{:?}", value).bright_cyan(),
		"i128" | "i16" | "i32" | "i64" | "i8" | "isize" => format_integer(value).bright_cyan(),
		"u128" | "u16" | "u32" | "u64" | "u8" | "usize" => format_integer(value).bright_cyan(),
		"alloc::string::String" | "str" | "&str" => format!("{:?}", value).bright_magenta(),
		_ => format!("{:?}", value).bright_green(),
	}
}

fn format_integer<V: Debug + ?Sized>(value: &V) -> String {
	let mut text = format!("{:?}", value);
	let mut formatted = String::from("");
	while (text.len() > 3) && text.chars().nth_back(3).unwrap().is_numeric() {
		let i = text.len() - 3;
		formatted = String::from("_") + &text[i..] + &formatted;
		text = String::from(&text[..i]);
	}
	if formatted.is_empty() {
		text
	} else {
		text + &formatted
	}
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
		cat.get_list("test_list_1")
			.await
			.add_key_value("string_1", &4)
			.await;
		cat.add_str(String::from("string_2")).await;
		cat.add_warning("test_warning_2").await;
		printer.add_warning("test_warning_3").await;

		let result = printer.as_string().await;
		assert_eq!(
			&result,
			"test_warning_1\ntest_category_1:\n   test_list_1:\n      string_1: 4\nstring_2   test_warning_2\ntest_warning_3\n"
		);
	}

	#[test]
	#[should_panic] // everybody should panic
	fn x() {
		assert_eq!("Elon Musk", "Genius");
	}
}
