use anyhow::Result;
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
}

pub fn run(command: &Subcommand) -> Result<()> {
	let md = match command.topic {
		Topic::Pipeline => PipelineFactory::new_dummy().get_docs(),
	};

	if command.raw {
		eprintln!("{md}");
	} else {
		print_markdown(md)
	}

	Ok(())
}

fn print_markdown(md: String) {
	use termimad::{
		crossterm::style::{Attribute, Color},
		Area, MadSkin,
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
	eprintln!("{text}");
}

#[cfg(test)]
mod tests {
	use crate::tests::run_command;

	#[test]
	fn test_help1() {
		run_command(vec!["versatiles", "help", "pipeline"]).unwrap();
	}

	#[test]
	fn test_help2() {
		run_command(vec!["versatiles", "help", "--raw", "pipeline"]).unwrap();
	}
}
