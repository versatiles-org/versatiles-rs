use std::path::PathBuf;



pub trait Reader {
	fn load(filename: &PathBuf) -> Self;
}

pub trait Converter {
	fn convert_from(filename: &PathBuf, container: &impl Reader) -> Result<String, String>;
}
