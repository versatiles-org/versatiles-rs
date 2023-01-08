use crate::{Probe, opencloudtiles::tools::get_reader};


pub fn probe(arguments: &Probe) {
	println!("probe {:?}", arguments.file);

	let reader = get_reader(&arguments.file);
	println!("{:#?}", reader);
}

