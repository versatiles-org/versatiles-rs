use colored::*;
use std::fmt::{Debug, Display};
use std::io::{stderr, Write};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct PrettyPrinter {
	indention: String,
	output: Arc<Mutex<Box<dyn Write + Send + Sync>>>,
}

impl PrettyPrinter {
	pub fn new_printer() -> PrettyPrint {
		let me = Arc::new(Self {
			indention: String::from("   "),
			output: Arc::new(Mutex::new(Box::new(stderr()))),
		});
		PrettyPrint::new(me, "\n")
	}
}
pub struct PrettyPrint {
	prefix: String,
	parent: Arc<PrettyPrinter>,
}

impl PrettyPrint {
	fn new(parent: Arc<PrettyPrinter>, indent: &str) -> Self {
		Self {
			prefix: String::from(indent),
			parent,
		}
	}
	fn new_indented(&mut self) -> Self {
		Self {
			prefix: String::from(&self.prefix) + &self.parent.indention,
			parent: self.parent.clone(),
		}
	}
	pub async fn get_category(&mut self, text: &str) -> PrettyPrint {
		self.write(format!("{}{}:", self.prefix, text.bold().white())).await;
		self.new_indented()
	}
	pub async fn get_list(&mut self, text: &str) -> PrettyPrint {
		self.write(format!("{}{}:", self.prefix, text.white())).await;
		self.new_indented()
	}
	pub async fn add_warning(&self, text: &str) {
		self.write(format!("{}{}", self.prefix, text.bold().yellow())).await;
	}
	pub async fn add_key_value<K: Display, V: Debug>(&self, key: &K, value: &V) {
		self
			.write(format!("{}{}: {}", self.prefix, key, get_formatted_value(value)))
			.await;
	}
	pub async fn add_value<V: Debug>(&self, value: &V) {
		self
			.write(format!("{}{}", self.prefix, get_formatted_value(value)))
			.await;
	}
	async fn write(&self, text: String) {
		self.parent.output.lock().await.write_all(text.as_bytes()).unwrap();
	}
}

fn get_formatted_value<V: Debug>(value: &V) -> ColoredString {
	let type_name = std::any::type_name::<V>();
	if type_name.starts_with("versatiles::shared::") {
		return format!("{:?}", value).bright_blue();
	}
	match type_name {
		"bool" => format!("{:?}", value).bright_green(),
		"f32" | "f64" => format!("{:?}", value).bright_cyan(),
		"i128" | "i16" | "i32" | "i64" | "i8" | "isize" => format_integer(value).bright_cyan(),
		"u128" | "u16" | "u32" | "u64" | "u8" | "usize" => format_integer(value).bright_cyan(),
		"str" | "&str" => format!("{:?}", value).bright_magenta(),
		_ => {
			panic!("Unknown typename {type_name}");
		}
	}
}

fn format_integer<V: Debug>(value: &V) -> String {
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
	use std::io::Cursor;
	use std::sync::Arc;
	use tokio::sync::Mutex;

	#[tokio::test]
	async fn test_new_printer() {
		let _printer = PrettyPrinter::new_printer();
	}

	#[tokio::test]
	async fn test_get_category() {
		let parent = Arc::new(PrettyPrinter {
			indention: String::from("   "),
			output: Arc::new(Mutex::new(Box::new(Cursor::new(vec![])))),
		});
		let mut pretty_print = PrettyPrint::new(parent.clone(), "\n");

		let _new_pretty_print = pretty_print.get_category("test category").await;
	}

	#[tokio::test]
	async fn test_get_list() {
		let parent = Arc::new(PrettyPrinter {
			indention: String::from("   "),
			output: Arc::new(Mutex::new(Box::new(Cursor::new(vec![])))),
		});
		let mut pretty_print = PrettyPrint::new(parent.clone(), "\n");

		let _new_pretty_print = pretty_print.get_list("test list").await;
	}

	#[tokio::test]
	async fn test_add_warning() {
		let parent = Arc::new(PrettyPrinter {
			indention: String::from("   "),
			output: Arc::new(Mutex::new(Box::new(Cursor::new(vec![])))),
		});
		let pretty_print = PrettyPrint::new(parent.clone(), "\n");

		pretty_print.add_warning("test warning").await;
	}

	#[tokio::test]
	async fn test_add_key_value() {
		let parent = Arc::new(PrettyPrinter {
			indention: String::from("   "),
			output: Arc::new(Mutex::new(Box::new(Cursor::new(vec![])))),
		});
		let pretty_print = PrettyPrint::new(parent.clone(), "\n");

		pretty_print.add_key_value(&"key", &"value").await;
	}
}
