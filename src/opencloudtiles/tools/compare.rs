use crate::{opencloudtiles::tools::get_reader, Compare};

pub fn compare(arguments: &Compare) {
	println!("compare {:?} with {:?}", arguments.file1, arguments.file2);

	let _reader1 = get_reader(&arguments.file1);
	let _reader2 = get_reader(&arguments.file2);
	todo!()
}
