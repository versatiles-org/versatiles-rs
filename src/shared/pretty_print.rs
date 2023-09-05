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
		self.write(format!("{}{}:", self.prefix, text)).await;
		self.new_indented()
	}
	pub async fn get_list(&mut self, text: &str) -> PrettyPrint {
		self.write(format!("{}{}:", self.prefix, text)).await;
		self.new_indented()
	}
	pub async fn add_warning(&self, text: &str) {
		self.write(format!("{}{}", self.prefix, text)).await;
	}
	pub async fn add_key_value<K: Display, V: Debug>(&self, key: &K, value: &V) {
		self.write(format!("{}{}: {:?}", self.prefix, key, value)).await;
	}
	async fn write(&self, text: String) {
		self.parent.output.lock().await.write_all(text.as_bytes()).unwrap();
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
