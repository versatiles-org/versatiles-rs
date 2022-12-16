use crate::tiles::container;

pub struct Reader;
impl container::Reader for Reader {
	fn load(filename: &std::path::PathBuf) -> Self {
		panic!("not implemented");
		return Reader;
	}
}

pub struct Converter;
impl container::Converter for Converter {
	fn convert_from(
		filename: &std::path::PathBuf,
		container: &impl container::Reader,
	) -> Result<String, String> {
		panic!("not implemented");
	}
}
