use anyhow::Result;
use versatiles::config::Config;
use versatiles_pipeline::PipelineFactory;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	#[command(subcommand)]
	topic: Topic,

	/// print raw markdown help without formatting
	#[arg(long)]
	raw: bool,
}

#[derive(clap::Subcommand, Debug)]
enum Topic {
	Pipeline,
	Config,
	Source,
}

/// Returns markdown help text explaining data source syntax.
fn data_source_help_md() -> String {
	include_str!("help_source.md").to_string()
}

pub fn run(command: &Subcommand) -> Result<()> {
	let md = match command.topic {
		Topic::Pipeline => PipelineFactory::new_dummy().help_md(),
		Topic::Config => Config::help_md(),
		Topic::Source => data_source_help_md(),
	};

	if command.raw {
		println!("{md}");
	} else {
		print_markdown(&md);
	}

	Ok(())
}

fn print_markdown(md: &str) {
	use termimad::{
		Area, MadSkin,
		crossterm::style::{Attribute, Color},
	};

	let mut skin = MadSkin::default();

	let color = |s: &str| {
		let rgb = s
			.trim_start_matches('#')
			.as_bytes()
			.chunks(1)
			.map(|char| u8::from_str_radix(std::str::from_utf8(char).unwrap(), 16).unwrap() * 17)
			.collect::<Vec<u8>>();
		Color::Rgb {
			r: rgb[0],
			g: rgb[1],
			b: rgb[2],
		}
	};

	// Configure header level 1
	skin.headers.get_mut(0).unwrap().set_fg(color("#D33"));

	// Configure header level 2
	let h2 = skin.headers.get_mut(1).unwrap();
	h2.set_fg(color("#D63"));
	h2.compound_style.add_attr(Attribute::Bold);
	h2.compound_style.style_char('#');

	// Configure header level 3
	skin.headers.get_mut(2).unwrap().set_fg(color("#DD8"));

	// Set the other text styles
	skin.bold.set_fg(color("#FFF"));
	skin.italic.set_fg(color("#FFF"));
	skin.inline_code.set_fgbg(color("#DDF"), color("#002"));
	skin.code_block.set_fgbg(color("#DDF"), color("#002"));

	// Ensure minimum dimensions for the area
	let mut area = Area::full_screen();
	area.width = area.width.max(50);
	area.height = area.height.max(20);

	let text = skin.area_text(md, &area);
	println!("{text}");
}

#[cfg(test)]
mod tests {
	use crate::tests::run_command;
	use anyhow::Result;

	#[test]
	fn test_help1() -> Result<()> {
		run_command(vec!["versatiles", "help", "pipeline"])?;
		Ok(())
	}

	#[test]
	fn test_help2() -> Result<()> {
		run_command(vec!["versatiles", "help", "--raw", "pipeline"])?;
		Ok(())
	}

	#[test]
	fn test_help_config1() -> Result<()> {
		run_command(vec!["versatiles", "help", "config"])?;
		Ok(())
	}

	#[test]
	fn test_help_config2() -> Result<()> {
		run_command(vec!["versatiles", "help", "--raw", "config"])?;
		Ok(())
	}

	#[test]
	fn test_help_source1() -> Result<()> {
		run_command(vec!["versatiles", "help", "source"])?;
		Ok(())
	}

	#[test]
	fn test_help_source2() -> Result<()> {
		run_command(vec!["versatiles", "help", "--raw", "source"])?;
		Ok(())
	}
}
