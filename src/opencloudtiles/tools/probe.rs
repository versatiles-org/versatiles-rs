use crate::{opencloudtiles::tools::get_reader, Probe};

pub fn probe(arguments: &Probe) {
	println!("probe {:?}", arguments.file);

	let reader = get_reader(&arguments.file);
	println!("{:#?}", reader);
}
