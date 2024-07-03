use crate::pipeline::PipelineFactory;
use anyhow::Result;
use std::path::Path;

#[derive(clap::Args, Debug)]
#[command(
	arg_required_else_help = true,
	disable_help_flag = true,
	disable_version_flag = true
)]
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
		Topic::Pipeline => PipelineFactory::default(Path::new(""), None).get_docs(),
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
	skin.headers.get_mut(0).unwrap().set_fg(Color::Yellow);

	let h2 = skin.headers.get_mut(1).unwrap();
	h2.set_fg(Color::Yellow);
	h2.compound_style.add_attr(Attribute::Bold);
	h2.compound_style.remove_attr(Attribute::Underlined);

	skin.headers.get_mut(2).unwrap().set_fg(Color::White);
	skin.bold.set_fg(Color::White);
	skin.italic.set_fg(Color::White);
	skin.inline_code.set_bg(Color::Reset);
	skin.inline_code.set_fg(Color::Green);

	let area = Area::full_screen();
	let text = skin.area_text(&md, &area);
	eprintln!("{text}");
}

#[cfg(test)]
mod test {
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
