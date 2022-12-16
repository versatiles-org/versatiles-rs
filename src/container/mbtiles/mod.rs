use crate::container::container;

pub struct Reader {}
impl container::Reader for Reader {
	fn load(filename: &std::path::PathBuf) -> Box<dyn container::Reader> {
		let reader = Reader {};
		return Box::new(reader);
	}
}

pub struct Converter;
impl container::Converter for Converter {}
