
use crate::container::container;

pub struct Reader;
impl container::Reader for Reader {
}

pub struct Converter;
impl container::Converter for Converter {
	fn convert_from(
		filename: &std::path::PathBuf,
		container: Box<dyn container::Reader>,
	) -> std::io::Result<()> {
		let mut file = std::fs::File::create(filename)?;
		panic!("not implemented");
	}
}
