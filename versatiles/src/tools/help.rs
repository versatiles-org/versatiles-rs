use anyhow::Result;
use versatiles::config::Config;
use versatiles_pipeline::PipelineFactory;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_help_flag = true, disable_version_flag = true)]
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
}

pub fn run(command: &Subcommand) -> Result<()> {
	let md = match command.topic {
		Topic::Pipeline => PipelineFactory::new_dummy().help_md(),
		Topic::Config => Config::help_md(),
	};

	if command.raw {
		println!("{md}");
	} else {
		print_markdown(md)
	}

	Ok(())
}

fn print_markdown(md: String) {
	use termimad::{
		Area, MadSkin,
		crossterm::style::{Attribute, Color},
	};

	let mut skin = MadSkin::default();

	// Configure header level 1
	skin.headers.get_mut(0).unwrap().set_fg(Color::Yellow);

	// Configure header level 2
	let h2 = skin.headers.get_mut(1).unwrap();
	h2.set_fg(Color::Yellow);
	h2.compound_style.add_attr(Attribute::Bold);
	h2.compound_style.remove_attr(Attribute::Underlined);

	// Configure header level 3
	skin.headers.get_mut(2).unwrap().set_fg(Color::White);

	// Set the other text styles
	skin.bold.set_fg(Color::White);
	skin.italic.set_fg(Color::White);
	skin.inline_code.set_bg(Color::Reset);
	skin.inline_code.set_fg(Color::Green);

	// Ensure minimum dimensions for the area
	let mut area = Area::full_screen();
	area.width = area.width.max(50);
	area.height = area.height.max(20);

	let text = skin.area_text(&md, &area);
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
}
