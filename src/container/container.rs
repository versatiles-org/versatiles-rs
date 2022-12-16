use std::path::PathBuf;



pub trait Reader {
	fn load(filename: &PathBuf) -> Box<dyn Reader> where Self: Sized {
		panic!("not implemented");
	}
}

pub trait Converter {
	fn convert_from(filename: &PathBuf, container: Box<dyn Reader>) -> std::io::Result<()> {
		panic!("not implemented");
	}
}
