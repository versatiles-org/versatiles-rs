//! CSV/TSV file reader with configurable separators.
//!
//! This module provides [`CsvReader`], a flexible reader for CSV and TSV files
//! that supports different regional formats commonly used in Europe.
//!
//! # Features
//!
//! - **Auto-detection**: Automatically uses tab separator for `.tsv` files
//! - **Configurable field separator**: Support for comma (`,`), semicolon (`;`), tab (`\t`), or any character
//! - **Configurable decimal separator**: Support for dot (`.`) or comma (`,`) in numeric values
//! - **Progress reporting**: Integrates with the tiles runtime for progress feedback
//!
//! # Examples
//!
//! ## Standard CSV (US/UK format)
//! ```ignore
//! let data = CsvReader::new(Path::new("data.csv"), runtime)
//!     .read()
//!     .await?;
//! ```
//!
//! ## German CSV (semicolon + comma decimal)
//! ```ignore
//! let data = CsvReader::new(Path::new("data.csv"), runtime)
//!     .with_field_separator(';')
//!     .with_decimal_separator(',')
//!     .read()
//!     .await?;
//! ```
//!
//! ## TSV file (auto-detected)
//! ```ignore
//! // Tab separator is auto-detected from .tsv extension
//! let data = CsvReader::new(Path::new("data.tsv"), runtime)
//!     .read()
//!     .await?;
//! ```

use anyhow::{Result, bail};
use std::{collections::HashSet, io::BufReader, path::Path};
use versatiles_container::TilesRuntime;
use versatiles_core::utils::read_csv_iter;
use versatiles_derive::context;
use versatiles_geometry::geo::*;

/// A configurable CSV/TSV file reader.
///
/// `CsvReader` reads delimited text files and converts each row into [`GeoProperties`],
/// which can then be used to update vector tile features.
///
/// The reader automatically detects TSV files by extension and supports configurable
/// field and decimal separators for international CSV formats.
///
/// # Example
///
/// ```ignore
/// use std::path::Path;
/// use versatiles_pipeline::helpers::CsvReader;
///
/// // Read a German CSV file with semicolon separator and comma decimals
/// let data = CsvReader::new(Path::new("german_data.csv"), runtime)
///     .with_field_separator(';')
///     .with_decimal_separator(',')
///     .read()
///     .await?;
///
/// // Each row becomes a GeoProperties map
/// for row in &data {
///     println!("{:?}", row.get("column_name"));
/// }
/// ```
#[derive(Clone)]
pub struct CsvReader {
	/// Field separator byte. Defaults to `b','` for `.csv` files and `b'\t'` for `.tsv` files.
	pub field_separator: u8,

	/// Decimal separator for parsing floating-point numbers.
	/// - `None` (default): Uses `.` as decimal separator
	/// - `Some(',')`: Uses `,` as decimal separator (common in German/French locales)
	pub decimal_separator: Option<char>,

	runtime: TilesRuntime,
	path: std::path::PathBuf,
	string_fields: HashSet<String>,
}

impl CsvReader {
	/// Creates a new `CsvReader` for the given file path.
	///
	/// The field separator is automatically detected based on the file extension:
	/// - `.tsv` files use tab (`\t`) as separator
	/// - All other files use comma (`,`) as separator
	///
	/// # Arguments
	///
	/// * `path` - Path to the CSV/TSV file to read
	/// * `runtime` - Tiles runtime for progress reporting
	///
	/// # Example
	///
	/// ```ignore
	/// let reader = CsvReader::new(Path::new("data.csv"), runtime);
	/// ```
	#[must_use]
	pub fn new(path: &Path, runtime: TilesRuntime) -> Self {
		// Auto-detect based on file extension
		let field_separator = if let Some(ext) = path.extension()
			&& ext.eq_ignore_ascii_case("tsv")
		{
			b'\t'
		} else {
			b','
		};

		Self {
			field_separator,
			decimal_separator: None,
			runtime,
			path: path.to_path_buf(),
			string_fields: HashSet::new(),
		}
	}

	/// Sets a custom field separator character.
	///
	/// Use this to override the auto-detected separator. Common values:
	/// - `,` (comma) - Standard CSV format (US/UK)
	/// - `;` (semicolon) - European CSV format (Germany, France)
	/// - `\t` (tab) - TSV format
	///
	/// # Example
	///
	/// ```ignore
	/// // Parse a German CSV file with semicolon separator
	/// let reader = CsvReader::new(path, runtime)
	///     .with_field_separator(';');
	/// ```
	#[must_use]
	pub fn with_field_separator(mut self, sep: char) -> Self {
		self.field_separator = sep as u8;
		self
	}

	/// Sets a custom decimal separator for parsing numbers.
	///
	/// By default, numbers are parsed with `.` as the decimal separator.
	/// Use this for files where numbers use `,` as the decimal separator
	/// (common in German, French, and other European locales).
	///
	/// # Example
	///
	/// ```ignore
	/// // Parse German numbers like "3,14" as 3.14
	/// let reader = CsvReader::new(path, runtime)
	///     .with_decimal_separator(',');
	/// ```
	///
	/// # Note
	///
	/// Setting the decimal separator to `.` is a no-op (keeps default behavior).
	#[must_use]
	pub fn with_decimal_separator(mut self, sep: char) -> Self {
		self.decimal_separator = if sep == '.' { None } else { Some(sep) };
		self
	}

	#[must_use]
	pub fn with_string_field(mut self, field_name: &str) -> Self {
		self.string_fields.insert(field_name.to_string());
		self
	}

	/// Converts a string value to a [`GeoValue`], applying decimal separator conversion if needed.
	fn convert_value(&self, value: &str) -> GeoValue {
		if let Some(decimal_sep) = self.decimal_separator {
			// Replace decimal separator with '.' for parsing
			let converted = value.replace(decimal_sep, ".");
			return GeoValue::parse_str(&converted);
		}
		GeoValue::parse_str(value)
	}

	/// Reads the CSV/TSV file and returns all rows as [`GeoProperties`].
	///
	/// The first row is treated as the header and defines the property names.
	/// Each subsequent row becomes a [`GeoProperties`] map where keys are
	/// column names and values are automatically parsed as numbers, booleans,
	/// or strings.
	///
	/// # Returns
	///
	/// A vector of [`GeoProperties`], one for each data row (excluding the header).
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The file cannot be opened
	/// - The file has inconsistent column counts
	/// - There are parsing errors
	///
	/// # Example
	///
	/// ```ignore
	/// let data = CsvReader::new(Path::new("data.csv"), runtime)
	///     .read()
	///     .await?;
	///
	/// for row in &data {
	///     if let Some(name) = row.get("name") {
	///         println!("Name: {name}");
	///     }
	/// }
	/// ```
	#[context("Failed to read CSV file at path: {:?}", self.path)]
	pub async fn read(&self) -> Result<Vec<GeoProperties>> {
		let file =
			std::fs::File::open(&self.path).with_context(|| format!("Failed to open file at path: {:?}", self.path))?;

		let size = file.metadata()?.len();
		let progress = self.runtime.create_progress("read csv", size);

		let reader = BufReader::new(file);

		let mut errors = vec![];
		let mut iter = read_csv_iter(reader, self.field_separator)?;
		let header: Vec<String> = iter.next().unwrap()?.0;
		let data: Vec<GeoProperties> = iter
			.filter_map(|e| {
				e.map(|(fields, _line_pos, byte_pos)| {
					progress.set_position(byte_pos as u64);

					fields
						.into_iter()
						.enumerate()
						.map(|(col, value)| {
							let key = header[col].clone();
							let value = if self.string_fields.contains(&key) {
								GeoValue::String(value)
							} else {
								self.convert_value(&value)
							};
							(key, value)
						})
						.collect()
				})
				.map_err(|e| errors.push(e))
				.ok()
			})
			.collect::<Vec<_>>();

		progress.finish();

		if !errors.is_empty() {
			eprintln!("{errors:?}");
			bail!("found {} error(s) while reading csv", errors.len());
		}

		Ok(data)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use assert_fs::NamedTempFile;
	use std::{fs::File, io::Write};

	fn make_temp_file(name: &str, content: &str) -> Result<NamedTempFile> {
		let temp_file = NamedTempFile::new(name)?;
		let mut file = File::create(&temp_file)?;
		writeln!(&mut file, "{content}")?;
		drop(file);
		Ok(temp_file)
	}

	fn make_temp_csv(content: &str) -> Result<NamedTempFile> {
		make_temp_file("test.csv", content)
	}

	fn runtime() -> TilesRuntime {
		TilesRuntime::new_silent()
	}

	#[tokio::test]
	async fn test_read_csv_file() -> Result<()> {
		let file_path =
			make_temp_csv("name,age,city\nJohn Doe,30,New York\nJane Smith,25,Los Angeles\nAlice Johnson,28,Chicago")?;
		let data = CsvReader::new(file_path.path(), runtime()).read().await?;

		assert_eq!(data.len(), 3);

		let john = &data[0];
		assert_eq!(john.get("name").unwrap(), &GeoValue::from("John Doe"));
		assert_eq!(john.get("age").unwrap(), &GeoValue::from(30));
		assert_eq!(john.get("city").unwrap(), &GeoValue::from("New York"));

		let jane = &data[1];
		assert_eq!(jane.get("name").unwrap(), &GeoValue::from("Jane Smith"));
		assert_eq!(jane.get("age").unwrap(), &GeoValue::from(25));
		assert_eq!(jane.get("city").unwrap(), &GeoValue::from("Los Angeles"));

		let alice = &data[2];
		assert_eq!(alice.get("name").unwrap(), &GeoValue::from("Alice Johnson"));
		assert_eq!(alice.get("age").unwrap(), &GeoValue::from(28));
		assert_eq!(alice.get("city").unwrap(), &GeoValue::from("Chicago"));

		Ok(())
	}

	#[tokio::test]
	async fn test_read_empty_csv_file() -> Result<()> {
		let file_path = make_temp_csv("name,age,city")?;
		let data = CsvReader::new(file_path.path(), runtime()).read().await?;
		assert!(data.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn test_read_csv_file_missing_values() -> Result<()> {
		let file_path = make_temp_csv("name,age,city\nJohn Doe,,New York\n,25,Los Angeles\nAlice Johnson,28,")?;

		let data = CsvReader::new(file_path.path(), runtime()).read().await?;

		assert_eq!(data.len(), 3);

		let john = &data[0];
		assert_eq!(john.get("name").unwrap(), &GeoValue::from("John Doe"));
		assert_eq!(john.get("age").unwrap(), &GeoValue::from(""));
		assert_eq!(john.get("city").unwrap(), &GeoValue::from("New York"));

		let jane = &data[1];
		assert_eq!(jane.get("name").unwrap(), &GeoValue::from(""));
		assert_eq!(jane.get("age").unwrap(), &GeoValue::from(25));
		assert_eq!(jane.get("city").unwrap(), &GeoValue::from("Los Angeles"));

		let alice = &data[2];
		assert_eq!(alice.get("name").unwrap(), &GeoValue::from("Alice Johnson"));
		assert_eq!(alice.get("age").unwrap(), &GeoValue::from(28));
		assert_eq!(alice.get("city").unwrap(), &GeoValue::from(""));

		Ok(())
	}

	#[tokio::test]
	async fn test_read_csv_file_incorrect_path() {
		let path = Path::new("non_existent.csv");
		let result = CsvReader::new(path, runtime()).read().await;
		assert!(result.is_err());
	}

	// ───────────────────────── TSV Tests ─────────────────────────

	#[tokio::test]
	async fn test_read_tsv_file_auto_detect() -> Result<()> {
		// TSV file should auto-detect tab separator based on .tsv extension
		let file_path = make_temp_file("test.tsv", "name\tage\tcity\nJohn\t30\tBerlin\nJane\t25\tMunich")?;
		let data = CsvReader::new(file_path.path(), runtime()).read().await?;

		assert_eq!(data.len(), 2);

		let john = &data[0];
		assert_eq!(john.get("name").unwrap(), &GeoValue::from("John"));
		assert_eq!(john.get("age").unwrap(), &GeoValue::from(30));
		assert_eq!(john.get("city").unwrap(), &GeoValue::from("Berlin"));

		let jane = &data[1];
		assert_eq!(jane.get("name").unwrap(), &GeoValue::from("Jane"));
		assert_eq!(jane.get("age").unwrap(), &GeoValue::from(25));
		assert_eq!(jane.get("city").unwrap(), &GeoValue::from("Munich"));

		Ok(())
	}

	#[tokio::test]
	async fn test_read_tsv_with_explicit_separator() -> Result<()> {
		// Even a .csv file can be parsed with tab separator if specified
		let file_path = make_temp_csv("name\tage\nAlice\t42")?;
		let data = CsvReader::new(file_path.path(), runtime())
			.with_field_separator('\t')
			.read()
			.await?;

		assert_eq!(data.len(), 1);
		assert_eq!(data[0].get("name").unwrap(), &GeoValue::from("Alice"));
		assert_eq!(data[0].get("age").unwrap(), &GeoValue::from(42));

		Ok(())
	}

	// ───────────────────────── Semicolon Separator Tests ─────────────────────────

	#[tokio::test]
	async fn test_read_csv_semicolon_separator() -> Result<()> {
		// German CSV files often use semicolon as field separator
		let file_path = make_temp_csv("name;age;city\nHans;35;Hamburg\nGreta;28;Frankfurt")?;
		let data = CsvReader::new(file_path.path(), runtime())
			.with_field_separator(';')
			.read()
			.await?;

		assert_eq!(data.len(), 2);

		let hans = &data[0];
		assert_eq!(hans.get("name").unwrap(), &GeoValue::from("Hans"));
		assert_eq!(hans.get("age").unwrap(), &GeoValue::from(35));
		assert_eq!(hans.get("city").unwrap(), &GeoValue::from("Hamburg"));

		let greta = &data[1];
		assert_eq!(greta.get("name").unwrap(), &GeoValue::from("Greta"));
		assert_eq!(greta.get("age").unwrap(), &GeoValue::from(28));
		assert_eq!(greta.get("city").unwrap(), &GeoValue::from("Frankfurt"));

		Ok(())
	}

	// ───────────────────────── Decimal Separator Tests ─────────────────────────

	#[tokio::test]
	async fn test_read_csv_german_decimal_separator() -> Result<()> {
		// German CSV files use comma as decimal separator and semicolon as field separator
		let file_path = make_temp_csv("name;price;quantity\nApfel;1,99;100\nBirne;2,49;50")?;
		let data = CsvReader::new(file_path.path(), runtime())
			.with_field_separator(';')
			.with_decimal_separator(',')
			.read()
			.await?;

		assert_eq!(data.len(), 2);

		let apfel = &data[0];
		assert_eq!(apfel.get("name").unwrap(), &GeoValue::from("Apfel"));
		// Price should be parsed as float with comma converted to dot
		assert_eq!(apfel.get("price").unwrap(), &GeoValue::Double(1.99));
		assert_eq!(apfel.get("quantity").unwrap(), &GeoValue::from(100));

		let birne = &data[1];
		assert_eq!(birne.get("name").unwrap(), &GeoValue::from("Birne"));
		assert_eq!(birne.get("price").unwrap(), &GeoValue::Double(2.49));
		assert_eq!(birne.get("quantity").unwrap(), &GeoValue::from(50));

		Ok(())
	}

	#[tokio::test]
	async fn test_read_csv_german_negative_numbers() -> Result<()> {
		// Test negative numbers with German decimal separator
		let file_path = make_temp_csv("item;change\nStock A;-3,5\nStock B;12,75")?;
		let data = CsvReader::new(file_path.path(), runtime())
			.with_field_separator(';')
			.with_decimal_separator(',')
			.read()
			.await?;

		assert_eq!(data.len(), 2);

		assert_eq!(data[0].get("change").unwrap(), &GeoValue::Double(-3.5));
		assert_eq!(data[1].get("change").unwrap(), &GeoValue::Double(12.75));

		Ok(())
	}

	#[tokio::test]
	async fn test_read_csv_federal_statistical_office_format() -> Result<()> {
		// Simulating format from Federal Statistical Office Germany (Destatis)
		// Uses semicolon field separator and comma decimal separator
		let content = "Bundesland;Einwohner;Fläche_km2\nBayern;13140183;70541,57\nNRW;17925570;34112,31";
		let file_path = make_temp_csv(content)?;
		let data = CsvReader::new(file_path.path(), runtime())
			.with_field_separator(';')
			.with_decimal_separator(',')
			.read()
			.await?;

		assert_eq!(data.len(), 2);

		let bayern = &data[0];
		assert_eq!(bayern.get("Bundesland").unwrap(), &GeoValue::from("Bayern"));
		assert_eq!(bayern.get("Einwohner").unwrap(), &GeoValue::from(13140183_u64));
		assert_eq!(bayern.get("Fläche_km2").unwrap(), &GeoValue::Double(70541.57));

		let nrw = &data[1];
		assert_eq!(nrw.get("Bundesland").unwrap(), &GeoValue::from("NRW"));
		assert_eq!(nrw.get("Einwohner").unwrap(), &GeoValue::from(17925570_u64));
		assert_eq!(nrw.get("Fläche_km2").unwrap(), &GeoValue::Double(34112.31));

		Ok(())
	}

	// ───────────────────────── CsvReader Unit Tests ─────────────────────────

	#[test]
	fn test_csv_reader_auto_detect_csv() {
		let path = Path::new("data.csv");
		let reader = CsvReader::new(path, runtime());
		assert_eq!(reader.field_separator, b',');
		assert!(reader.decimal_separator.is_none());
	}

	#[test]
	fn test_csv_reader_auto_detect_tsv() {
		let path = Path::new("data.tsv");
		let reader = CsvReader::new(path, runtime());
		assert_eq!(reader.field_separator, b'\t');
	}

	#[test]
	fn test_csv_reader_builder() {
		let path = Path::new("data.csv");
		let reader = CsvReader::new(path, runtime())
			.with_field_separator(';')
			.with_decimal_separator(',');
		assert_eq!(reader.field_separator, b';');
		assert_eq!(reader.decimal_separator, Some(','));
	}

	#[test]
	fn test_csv_reader_explicit_overrides_extension() {
		let path = Path::new("data.tsv"); // Would normally be tab-separated
		let reader = CsvReader::new(path, runtime()).with_field_separator(';');
		assert_eq!(reader.field_separator, b';');
	}

	#[test]
	fn test_csv_reader_convert_value_default() {
		let path = Path::new("data.csv");
		let reader = CsvReader::new(path, runtime());
		assert_eq!(reader.convert_value("1.618"), GeoValue::Double(1.618));
		assert_eq!(reader.convert_value("42"), GeoValue::from(42));
		assert_eq!(reader.convert_value("hello"), GeoValue::from("hello"));
	}

	#[test]
	fn test_csv_reader_convert_value_german_decimal() {
		let path = Path::new("data.csv");
		let reader = CsvReader::new(path, runtime()).with_decimal_separator(',');
		assert_eq!(reader.convert_value("1,618"), GeoValue::Double(1.618));
		assert_eq!(reader.convert_value("-2,5"), GeoValue::Double(-2.5));
		assert_eq!(reader.convert_value("42"), GeoValue::from(42)); // Integers unaffected
		assert_eq!(reader.convert_value("hello"), GeoValue::from("hello")); // Strings unaffected
	}

	#[test]
	fn test_csv_reader_decimal_dot_is_normalized() {
		let path = Path::new("data.csv");
		// Setting decimal separator to '.' should result in None (default behavior)
		let reader = CsvReader::new(path, runtime()).with_decimal_separator('.');
		assert!(reader.decimal_separator.is_none());
	}
}
