use anyhow::Result;
use versatiles_container::get_reader;
use versatiles_core::ProbeDepth;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// tile container you want to probe
	/// supported container formats are: *.versatiles, *.tar, *.pmtiles, *.mbtiles or a directory
	#[arg(required = true, verbatim_doc_comment)]
	filename: String,

	/// deep scan (depending on the container implementation)
	///   -d: scans container
	///  -dd: scans all tiles
	/// -ddd: scans all tile contents
	#[arg(long, short, action = clap::ArgAction::Count, verbatim_doc_comment)]
	deep: u8,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand) -> Result<()> {
	eprintln!("probe {:?}", arguments.filename);

	log::debug!("open {:?}", arguments.filename);
	let mut reader = get_reader(&arguments.filename).await?;

	let level = match arguments.deep {
		0 => ProbeDepth::Shallow,
		1 => ProbeDepth::Container,
		2 => ProbeDepth::Tiles,
		3..=255 => ProbeDepth::TileContents,
	};

	log::debug!("probing {:?} at depth {:?}", arguments.filename, level);
	reader.probe(level).await?;

	Ok(())
}

#[cfg(test)]
mod tests {
	use crate::tests::run_command;
	use anyhow::Result;

	#[test]
	fn test_local() -> Result<()> {
		run_command(vec!["versatiles", "probe", "-q", "../testdata/berlin.mbtiles"])?;
		Ok(())
	}

	#[test]
	fn test_remote() -> Result<()> {
		run_command(vec![
			"versatiles",
			"probe",
			"-q",
			"https://download.versatiles.org/osm.versatiles",
		])?;
		Ok(())
	}
}
