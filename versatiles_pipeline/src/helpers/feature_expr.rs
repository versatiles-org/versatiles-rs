//! Feature expressions: the CEL predicates operations evaluate per feature.
//!
//! `vector_filter_features` was the only operation that tested features, so
//! this lived inside it, and `vector_reduce_to_style` borrowed its runner. It is
//! shared now because more operations need the same thing: an expression that
//! compiles the same way, reserves the same names and binds them the same way
//! wherever it appears.

use std::{collections::HashMap, sync::Arc};

use anyhow::{Result, anyhow};
use cel_interpreter::{
	Context as CelContext, ParseErrors, Program, Value as CelValue,
	extractors::This,
	objects::{Key as CelKey, Map as CelMap},
};
use versatiles_core::TileCoord;
use versatiles_geometry::{
	geo::{GeoProperties, GeoValue},
	vector_tile::{GeomType, VectorTileFeature},
};

use super::expression_tokens::{
	ExpressionToken, ExpressionTokenKind, is_identifier_char, is_identifier_start, is_string_prefix, lex, string_end,
};

/// A CEL expression, compiled where the value is decoded.
///
/// An expression that does not compile used to be found when the operation was
/// built; `check` finds it now, without building anything (#260). The text is
/// kept rather than the [`Program`], which is not `Clone`; both this and
/// [`FeatureExpr::new`] call [`compile_cel`], so what `check` accepts and what
/// builds are the same set.
#[derive(Clone, Debug)]
pub struct CelExpression(String);

impl CelExpression {
	/// The expression as it was written.
	#[must_use]
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl TryFrom<&str> for CelExpression {
	type Error = anyhow::Error;

	fn try_from(expr: &str) -> Result<Self> {
		compile_cel(expr)?;
		Ok(Self(expr.to_string()))
	}
}

/// Something wrong with an expression, and where in it.
///
/// `start` and `end` count characters (Unicode scalar values) of the text as
/// written, `end` exclusive, so a `$` name counts with its `$`, as the tokens
/// of [`check_expression`](crate::check_expression) do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpressionProblem {
	/// What is wrong, without the expression or the position.
	pub message: String,
	/// The character offset where the problem starts.
	pub start: usize,
	/// The character offset just past it. Equal to `start` where something is
	/// missing, such as the value after a trailing `==`.
	pub end: usize,
}

impl ExpressionProblem {
	fn new(start: usize, end: usize, message: impl Into<String>) -> Self {
		ExpressionProblem {
			message: message.into(),
			start,
			end,
		}
	}
}

impl std::fmt::Display for ExpressionProblem {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.message)
	}
}

impl std::error::Error for ExpressionProblem {}

/// Compiles a CEL expression, with its problems as a terminal shows them.
///
/// The terminal's view of [`compile_checked`]: the expression, then each
/// problem with the line it is on and a caret under the column.
fn compile_cel(expr: &str) -> Result<Program> {
	compile_checked(expr).map_err(|problems| anyhow!(render_problems(expr, &problems)))
}

/// Compiles a CEL expression, or says what is wrong with it and where.
///
/// The one compiler behind building, `check`, and
/// [`check_expression`](crate::check_expression), so the three accept the
/// same set.
///
/// ## Why it contains the parser's panics
///
/// `cel-parser 0.10.1` panics on some malformed inputs instead of returning
/// `ParseErrors`. Containing the panic keeps a bad expression from taking down
/// the whole pipeline — and also from taking down an editor asking `check`
/// what is wrong with the line someone is halfway through typing. The endings
/// known to panic are recognised before parsing (#325), so this is only for
/// whatever else still reaches it.
///
/// The recovery is silent to the caller but not to the terminal: `catch_unwind`
/// still runs the panic hook, so a panicking expression prints
/// `thread … panicked at …` to stderr before the error is returned. Suppressing
/// that would mean installing a process-global panic hook from inside a
/// library, which is the shape of problem #261 was filed about. Whoever owns
/// the process can set a hook; this function will not.
///
/// ## Why it parses on its own thread
///
/// `cel-parser`'s grammar is ANTLR-generated recursive descent, so its stack appetite grows with
/// the expression — and an expression can be generated rather than typed. `vector_reduce_to_style`
/// renders one term per drawn case, which for a real style is tens of kilobytes of CEL.
///
/// Compiling that in place means the parser gets whatever the caller happens to have left, and a
/// pipeline builds several frames deep inside an async runtime. On Windows, where the main thread
/// gets 1 MB against Linux's 8, that was not enough: `versatiles reduce` aborted with
/// `thread 'main' has overflowed its stack`, inside `CELParser::member_rec`.
///
/// Parsing on a thread with a stack of its own makes the answer depend on the expression alone
/// rather than on how deep the caller was. It costs one thread per compiled expression, which is a
/// build-time cost paid once per operation, never per tile.
pub(crate) fn compile_checked(expr: &str) -> std::result::Result<Program, Vec<ExpressionProblem>> {
	/// Generous, and deliberately so: it is reserved address space rather than committed memory,
	/// it lives only for the parse, and the failure it prevents is an abort rather than an error.
	const PARSER_STACK: usize = 16 * 1024 * 1024;

	// Checked here, at build time, because the stack the compiler gets is not the
	// stack the evaluator gets. Everything below protects the *parse*: its own
	// thread, 16 MB, `catch_unwind`. `Program::execute` then runs per feature on
	// the tiling thread, with none of that, and recurses over the same tree — so
	// an expression deep enough to trouble the parser aborts the process later,
	// during a conversion, where nothing catches it.
	//
	// Refusing it before it is ever evaluated turns that into an error at
	// pipeline build, which is also when a person is best placed to fix it.
	ensure_cel_nesting_is_bounded(expr).map_err(|problem| vec![problem])?;

	let compiled_text = rewrite_special_names(expr).map_err(|problem| vec![problem])?;

	// The parser panics on these, so they never reach it (#325).
	if let Some(problem) = unknown_character(expr).or_else(|| unfinished_ending(expr)) {
		return Err(vec![problem]);
	}

	// `ParseErrors` holds an `Rc` and so cannot cross a thread boundary; it is turned into
	// problems here, inside the thread.
	let parse = || {
		std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
			Program::compile(&compiled_text).map_err(|errors| parse_problems(&errors, expr))
		}))
	};

	// `scope` so the closure may borrow `expr` instead of forcing an allocation to satisfy
	// `'static`. A thread that cannot be spawned is not fatal — parsing in place is what this did
	// before, and it is still right for every expression small enough to have been fine anyway.
	let compiled = std::thread::scope(|scope| {
		match std::thread::Builder::new()
			.stack_size(PARSER_STACK)
			.spawn_scoped(scope, parse)
		{
			Ok(handle) => handle.join().unwrap_or_else(Err),
			Err(_) => parse(),
		}
	});

	let program = compiled.map_err(|panic_payload| {
		// `catch_unwind` returns `Box<dyn Any + Send>`; extract a message if the panic
		// carried one (Rust panics with string literals or `format!`-ed Strings).
		let detail = panic_payload
			.downcast_ref::<String>()
			.map(String::as_str)
			.or_else(|| panic_payload.downcast_ref::<&'static str>().copied())
			.unwrap_or("no panic message");
		vec![ExpressionProblem::new(
			0,
			expr.chars().count(),
			format!(
				"the parser crashed on this expression ({detail}). Common causes are unmatched brackets, \
				 trailing operators and unsupported tokens."
			),
		)]
	})??;

	// Checked on the compiled program rather than by the scanner, because only
	// the parser knows which identifiers are variables: `$props.zoom` is fine.
	let refs = program.references();
	for name in FORMER_SPECIAL_NAMES {
		if refs.has_variable(name) {
			let (start, end) = variable_span(expr, name).unwrap_or((0, expr.chars().count()));
			return Err(vec![ExpressionProblem::new(
				start,
				end,
				format!("`{name}` is written `${name}` now. A feature property named {name} is `$props.{name}`."),
			)]);
		}
	}

	Ok(program)
}

/// How deeply brackets may nest inside a CEL expression.
///
/// Bounds the shape that makes evaluation recurse: `((((…))))`. It deliberately
/// does *not* bound expression *length* or operator chains — `a || b || c || …`
/// parses left-leaning, so its evaluation depth grows with the number of terms,
/// and that is exactly what `vector_reduce_to_style` generates: one
/// parenthesised term per drawn case, joined with `||`, tens of kilobytes for a
/// real style. Those have a nesting depth of about three however long they get,
/// and they work today, so capping chain length would break generated pipelines
/// to defend against a shape they do not have.
const MAX_CEL_NESTING_DEPTH: usize = 128;

/// Refuse an expression whose brackets nest deeper than the evaluator will go.
///
/// Brackets inside string literals are ignored: a generated style embeds
/// property values verbatim, and a value containing `(` is not nesting.
fn ensure_cel_nesting_is_bounded(expr: &str) -> std::result::Result<(), ExpressionProblem> {
	let mut depth = 0usize;
	let mut deepest = 0usize;
	// The first bracket that goes deeper than allowed.
	let mut too_deep: Option<usize> = None;
	let mut quote: Option<char> = None;
	let mut escaped = false;

	for (index, c) in expr.chars().enumerate() {
		if let Some(q) = quote {
			if escaped {
				escaped = false;
			} else if c == '\\' {
				escaped = true;
			} else if c == q {
				quote = None;
			}
			continue;
		}

		match c {
			'\'' | '"' => quote = Some(c),
			'(' | '[' | '{' => {
				depth += 1;
				deepest = deepest.max(depth);
				if depth > MAX_CEL_NESTING_DEPTH && too_deep.is_none() {
					too_deep = Some(index);
				}
			}
			')' | ']' | '}' => depth = depth.saturating_sub(1),
			_ => {}
		}
	}

	match too_deep {
		None => Ok(()),
		Some(index) => Err(ExpressionProblem::new(
			index,
			index + 1,
			format!("the expression nests brackets {deepest} deep, more than the {MAX_CEL_NESTING_DEPTH} this evaluates"),
		)),
	}
}

/// A name an expression reads besides a feature's own properties, written
/// with a leading `$`.
///
/// The `$` is what keeps them apart from properties. A property is a top-level
/// variable only if its name is a CEL identifier, and no identifier starts with
/// `$`, so no property can ever hide one of these or be hidden by one. They used
/// to be bare words, and `zoom` and `props` shadowed any property of that name.
///
/// CEL has no `$` in identifiers, so each `$` becomes a `_` before compiling:
/// `$zoom` is compiled as `_zoom`. The compiled text is then exactly as long as
/// the one written, so a parser error points at the right column. That needs
/// [`rewrite_special_names`] to refuse a bare `_zoom`, which would otherwise
/// read the zoom level without saying so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Special {
	/// `$zoom`. See [`Special::doc`] for each name's meaning.
	Zoom,
	/// `$x`.
	X,
	/// `$y`.
	Y,
	/// `$layer`.
	Layer,
	/// `$type`.
	Type,
	/// `$id`.
	Id,
	/// `$props`.
	Props,
}

impl Special {
	const ALL: [Special; 7] = [
		Special::Zoom,
		Special::X,
		Special::Y,
		Special::Layer,
		Special::Type,
		Special::Id,
		Special::Props,
	];

	/// The name as written, `$` included.
	const fn written(self) -> &'static str {
		match self {
			Special::Zoom => "$zoom",
			Special::X => "$x",
			Special::Y => "$y",
			Special::Layer => "$layer",
			Special::Type => "$type",
			Special::Id => "$id",
			Special::Props => "$props",
		}
	}

	/// The name as written after the `$`.
	fn name(self) -> &'static str {
		&self.written()[1..]
	}

	/// What the name's value is, in one sentence, for an editor to show.
	const fn doc(self) -> &'static str {
		match self {
			Special::Zoom => "The zoom level of the tile.",
			Special::X => "The column of the tile.",
			Special::Y => "The row of the tile.",
			Special::Layer => "The name of the feature's layer.",
			Special::Type => "The feature's geometry type: 'Point', 'LineString' or 'Polygon', as in MapLibre.",
			Special::Id => "The feature's id, or null if it has none.",
			Special::Props => "All of the feature's properties, as a map: $props.name is the same as name.",
		}
	}

	/// The identifier it is compiled as.
	fn var(self) -> &'static str {
		match self {
			Special::Zoom => "_zoom",
			Special::X => "_x",
			Special::Y => "_y",
			Special::Layer => "_layer",
			Special::Type => "_type",
			Special::Id => "_id",
			Special::Props => "_props",
		}
	}

	/// How much of a tile the name's value depends on.
	fn scope(self) -> ExpressionScope {
		match self {
			Special::Zoom | Special::X | Special::Y => ExpressionScope::Tile,
			Special::Layer => ExpressionScope::Layer,
			Special::Type | Special::Id | Special::Props => ExpressionScope::Feature,
		}
	}

	fn from_name(name: &str) -> Option<Special> {
		Special::ALL.into_iter().find(|special| special.name() == name)
	}

	fn from_var(var: &str) -> Option<Special> {
		Special::ALL.into_iter().find(|special| special.var() == var)
	}

	/// Every name, as a reader would list them: "`$zoom`, `$x`, … and `$props`".
	fn listed() -> String {
		let names: Vec<String> = Special::ALL
			.iter()
			.map(|special| format!("`${}`", special.name()))
			.collect();
		let (last, rest) = names.split_last().expect("there are special names");
		format!("{} and {last}", rest.join(", "))
	}
}

/// Every `$` name with its doc, in [`Special::ALL`]'s order: the table
/// [`expression_names`] hands out, built from the one the parser reads.
const EXPRESSION_NAMES: [(&str, &str); Special::ALL.len()] = {
	let mut names = [("", ""); Special::ALL.len()];
	let mut index = 0;
	while index < names.len() {
		names[index] = (Special::ALL[index].written(), Special::ALL[index].doc());
		index += 1;
	}
	names
};

/// Every name a feature expression can read besides a feature's own
/// properties, `$` included, with what its value is.
///
/// For an editor offering the names as completions: these are the names that
/// compile, because the parser reads the same table, so a name added or
/// renamed here reaches the editor without a copy to update. The names are
/// those of `vector_branch expr=` and `vector_filter_features expr=`.
#[must_use]
pub fn expression_names() -> &'static [(&'static str, &'static str)] {
	&EXPRESSION_NAMES
}

/// The bare words that meant `$zoom` and `$props` before the `$` was required.
///
/// Refused rather than read as properties, so a pipeline written for the old
/// meaning fails with a pointer to the new spelling instead of quietly
/// comparing a property that no feature has.
const FORMER_SPECIAL_NAMES: [&str; 2] = ["zoom", "props"];

/// Rewrites each `$name` in `expr` into the identifier it is compiled as.
///
/// Only code is rewritten. String literals, in every form CEL has, and `//`
/// comments are copied as they are, so `name == 'costs $zoom'` compares against
/// that text.
///
/// # Errors
///
/// For a `$` not followed by the name of a [`Special`], and for a bare
/// identifier that is one of the compiled names, such as `_zoom`.
fn rewrite_special_names(expr: &str) -> std::result::Result<String, ExpressionProblem> {
	let chars: Vec<char> = expr.chars().collect();
	let mut out = String::with_capacity(expr.len());
	// The last character of code that is not white space. After a `.`, a name
	// is a field, as in `$props._zoom`, not a variable.
	let mut previous: Option<char> = None;
	let mut i = 0;

	while i < chars.len() {
		let start = i;
		let c = chars[i];

		if c == '/' && chars.get(i + 1) == Some(&'/') {
			// A comment: copied, and not code, so it leaves `previous` alone.
			while i < chars.len() && chars[i] != '\n' {
				i += 1;
			}
			out.extend(&chars[start..i]);
			continue;
		}

		if c == '$' {
			let name: String = chars[i + 1..].iter().take_while(|c| is_identifier_char(**c)).collect();
			let end = i + 1 + name.len();
			if Special::from_name(&name).is_none() {
				return Err(ExpressionProblem::new(
					i,
					end,
					format!(
						"`${name}` is not a name an expression can read. The names starting with `$` are {}; \
						 a feature property is written without one.",
						Special::listed()
					),
				));
			}
			if previous == Some('.') {
				return Err(ExpressionProblem::new(
					i,
					end,
					format!("`${name}` cannot follow a `.`: the names starting with `$` stand on their own."),
				));
			}
			out.push('_');
			out.push_str(&name);
			i = end;
			previous = name.chars().last();
			continue;
		}

		if c == '\'' || c == '"' {
			i = string_end(&chars, i, false).unwrap_or(chars.len());
		} else if c.is_ascii_digit() {
			// A number with its suffix or exponent, such as `1000u` or `1e5`, so
			// the letters in it are not taken for a name.
			while i < chars.len() && is_identifier_char(chars[i]) {
				i += 1;
			}
		} else if is_identifier_start(c) {
			while i < chars.len() && is_identifier_char(chars[i]) {
				i += 1;
			}
			let word: String = chars[start..i].iter().collect();
			if matches!(chars.get(i), Some('\'' | '"')) && is_string_prefix(&word) {
				i = string_end(&chars, i, word.contains(['r', 'R'])).unwrap_or(chars.len());
			} else if previous != Some('.') && Special::from_var(&word).is_some() {
				return Err(ExpressionProblem::new(
					start,
					i,
					format!(
						"`{word}` is reserved, because `${}` is compiled to it. \
						 For a feature property of that name, use `$props.{word}`.",
						&word[1..]
					),
				));
			}
		} else {
			i += 1;
		}

		out.extend(&chars[start..i]);
		if let Some(last) = chars[start..i].iter().rev().find(|c| !c.is_whitespace()) {
			previous = Some(*last);
		}
	}

	Ok(out)
}

/// The first character no expression can contain, such as `@`, or `None`.
///
/// The parser reports one with a position where it can, and panics where it
/// can't, as in `kind == @`. No expression can contain one, so it is refused
/// here and never reaches the parser.
fn unknown_character(expr: &str) -> Option<ExpressionProblem> {
	let token = lex(expr)
		.into_iter()
		.map(|lexed| lexed.token)
		.find(|token| token.kind == ExpressionTokenKind::Unknown)?;
	let character: String = expr.chars().skip(token.start).take(1).collect();
	Some(ExpressionProblem::new(
		token.start,
		token.end,
		format!("Syntax error: `{character}` cannot appear in an expression"),
	))
}

/// An expression that ends where nothing can end, or `None` for any other.
///
/// `cel-parser` 0.10 panics on an expression that ends in an operator, such
/// as `$layer ==`, after the `:` of a conditional or a map entry, or inside a
/// string, such as `kind == 'a`, where other unfinished expressions get a
/// syntax error with a position. Those endings are the common case while
/// typing: an editor that checks an expression as it is written meets one
/// every time someone pauses before the right-hand side. And the panic,
/// though caught, still runs the panic hook, which prints it to whatever
/// terminal or log hosts the pipeline. So they are recognised here, and the
/// parser never sees them.
///
/// Comments are skipped, and an operator inside a string is part of the string.
fn unfinished_ending(expr: &str) -> Option<ExpressionProblem> {
	let last = lex(expr)
		.into_iter()
		.rev()
		.find(|lexed| lexed.token.kind != ExpressionTokenKind::Comment)?;
	let token = last.token;
	if token.kind == ExpressionTokenKind::String && !last.closed {
		return Some(ExpressionProblem::new(
			token.start,
			token.end,
			"Syntax error: the string that starts here is never closed",
		));
	}
	if token.kind != ExpressionTokenKind::Operator {
		return None;
	}
	let operator: String = expr.chars().skip(token.start).take(token.end - token.start).collect();
	let needs = match operator.as_str() {
		"==" | "!=" | "<" | "<=" | ">" | ">=" => "something to compare with",
		"&&" | "||" => "a second condition",
		"!" => "a condition to negate",
		"in" => "a list or a map to look in",
		"+" | "-" | "*" | "/" | "%" | ":" => "a value after it",
		// The parser reports these itself.
		_ => return None,
	};
	// Nothing is wrong with the operator; what is missing is what comes after it.
	Some(ExpressionProblem::new(
		token.end,
		token.end,
		format!("Syntax error: the expression ends after `{operator}`, which needs {needs}"),
	))
}

/// The problems `cel-parser` reports, placed in the text as written.
///
/// The parser saw the compiled text, in which each `$` is a `_`, so its
/// columns are those of the written text, and only a name it quotes has to
/// be written back.
fn parse_problems(errors: &ParseErrors, expr: &str) -> Vec<ExpressionProblem> {
	let chars: Vec<char> = expr.chars().collect();
	let tokens = lex(expr);
	errors
		.errors
		.iter()
		.map(|error| {
			let start = offset_of(&chars, error.pos.0, error.pos.1);
			// The parser names a place; the token there is what it is about.
			let end = tokens
				.iter()
				.find(|lexed| lexed.token.start == start)
				.map_or(start, |lexed| lexed.token.end);
			let mut message = error.msg.clone();
			for special in Special::ALL {
				message = message.replace(&format!("'{}'", special.var()), &format!("'{}'", special.written()));
			}
			ExpressionProblem::new(start, end, message)
		})
		.collect()
}

/// The character offset of a 1-based line and column, as the parser counts them.
fn offset_of(chars: &[char], line: isize, column: isize) -> usize {
	let (Ok(line), Ok(column)) = (usize::try_from(line), usize::try_from(column)) else {
		return 0;
	};
	let mut line_start = 0;
	for _ in 1..line {
		match chars[line_start..].iter().position(|c| *c == '\n') {
			Some(newline) => line_start += newline + 1,
			None => return chars.len(),
		}
	}
	(line_start + column.saturating_sub(1)).min(chars.len())
}

/// Where the property `name` is read as a variable: not after a `.`, where it
/// is a field.
fn variable_span(expr: &str, name: &str) -> Option<(usize, usize)> {
	let chars: Vec<char> = expr.chars().collect();
	let text = |token: &ExpressionToken| chars[token.start..token.end].iter().collect::<String>();
	let mut after_dot = false;
	for lexed in lex(expr) {
		let token = lexed.token;
		if token.kind == ExpressionTokenKind::Comment {
			continue;
		}
		if !after_dot && token.kind == ExpressionTokenKind::Property && text(&token) == name {
			return Some((token.start, token.end));
		}
		after_dot = token.kind == ExpressionTokenKind::Punctuation && text(&token) == ".";
	}
	None
}

/// The problems as a terminal shows them: the expression, then each problem
/// in the layout `cel-parser` uses for its own errors, with the line it is on
/// and a caret under the column.
fn render_problems(expr: &str, problems: &[ExpressionProblem]) -> String {
	let chars: Vec<char> = expr.chars().collect();
	let reports: Vec<String> = problems
		.iter()
		.map(|problem| {
			let index = problem.start.min(chars.len());
			let line_start = chars[..index].iter().rposition(|c| *c == '\n').map_or(0, |n| n + 1);
			let line_end = chars[line_start..]
				.iter()
				.position(|c| *c == '\n')
				.map_or(chars.len(), |n| line_start + n);
			let line_number = chars[..index].iter().filter(|c| **c == '\n').count() + 1;
			let column = index - line_start + 1;
			let text: String = chars[line_start..line_end].iter().collect();
			let dots = ".".repeat(column - 1);
			format!(
				"ERROR: <input>:{line_number}:{column}: {}\n| {text}\n| {dots}^",
				problem.message
			)
		})
		.collect();
	format!("Failed to compile CEL expression:\n  {expr}\n\n{}", reports.join("\n"))
}

/// The name of the numeric type test this operation adds to CEL.
const IS_NUM_FN: &str = "is_num";

/// Whether a value is a number, as a CEL function.
///
/// CEL has no way to ask. `type()` is not implemented by `cel-interpreter 0.10`, there is no `is`
/// operator, and every other route — `int()`, `double()` — answers by failing, which is no answer
/// at all here: an evaluation error drops the feature, so asking costs exactly what it was meant
/// to avoid.
///
/// That matters because ordering comparisons are the one family that errors on a type mismatch —
/// `==`, `!=` and `in` return `false` instead — so a property that is a number in most tiles and
/// a string in one makes `population >= 1000` silently drop features in that tile. Guarding with
/// `has()` does not help: `&&` short-circuits only when the left side is `false`, and a property
/// that is *present* and mistyped passes that guard.
///
/// This is an extension, not standard CEL. It earns that by being the only total way to write a
/// safe ordering comparison, which is what a generated pipeline needs to keep its promise never
/// to drop a feature the style draws.
fn is_num(This(this): This<CelValue>) -> bool {
	matches!(this, CelValue::Int(_) | CelValue::UInt(_) | CelValue::Float(_))
}

/// How much of a tile an expression's answer depends on, narrowest first.
///
/// An expression whose answer is the same for a whole tile or layer is
/// evaluated once for it, without decoding its features.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExpressionScope {
	/// One answer for the whole tile: the expression reads at most `$zoom`, `$x` and `$y`.
	Tile,
	/// One answer per layer: it also reads `$layer`.
	Layer,
	/// An answer per feature: it reads a property, `$props`, `$type` or `$id`.
	Feature,
}

/// What a compiled expression reads, and so how much of a tile its answer
/// depends on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpressionReads {
	/// The `$` names it reads, `$` included, in the order of [`expression_names`].
	pub names: Vec<&'static str>,
	/// The feature properties it reads by their own names, sorted. One read
	/// through `$props`, such as `$props.name`, is not listed; `$props` is.
	pub properties: Vec<String>,
	/// How much of a tile its answer depends on.
	pub scope: ExpressionScope,
}

/// What `program` reads: its `$` names, and the properties it reads by name, sorted.
fn names_read(program: &Program) -> (Vec<Special>, Vec<String>) {
	let refs = program.references();
	let specials: Vec<Special> = Special::ALL
		.into_iter()
		.filter(|special| refs.has_variable(special.var()))
		.collect();
	let mut properties: Vec<String> = refs
		.variables()
		.into_iter()
		.filter(|v| Special::from_var(v).is_none())
		.map(String::from)
		.collect();
	properties.sort_unstable();
	(specials, properties)
}

/// The widest [`Special::scope`] of the names read, or a feature's when a
/// property is read.
fn scope_of(specials: &[Special], properties: &[String]) -> ExpressionScope {
	if !properties.is_empty() {
		return ExpressionScope::Feature;
	}
	specials
		.iter()
		.map(|special| special.scope())
		.max()
		.unwrap_or(ExpressionScope::Tile)
}

/// What a compiled program reads, as [`ExpressionReads`].
pub(crate) fn reads_of(program: &Program) -> ExpressionReads {
	let (specials, properties) = names_read(program);
	ExpressionReads {
		names: specials.iter().map(|special| special.written()).collect(),
		scope: scope_of(&specials, &properties),
		properties,
	}
}

/// A compiled feature expression, evaluated against one feature at a time.
///
/// Knows which names the expression reads, so each evaluation binds only those:
/// the [`Special`] names it uses, and the feature properties it mentions.
pub(crate) struct FeatureExpr {
	program: Program,
	/// The functions every evaluation needs, registered once.
	///
	/// `Context::default()` installs twelve builtins — `size`, `string`, `int`, `matches` and the
	/// rest — and `evaluate` used to build one per feature, so a tile with 5,000 features paid
	/// 60,000 registrations to answer one predicate. Building it here and deriving a child scope
	/// per feature costs one `HashMap` instead.
	///
	/// `is_num` is registered here and *only* here: `Context::add_function` silently does nothing
	/// on a child scope (`if let Context::Root { .. } = self`), so registering it per feature
	/// would leave the expression calling an undeclared function, which errors, which drops every
	/// feature. Silently.
	context: CelContext<'static>,
	/// Top-level identifiers referenced by the expression, other than the special names.
	/// Bound to the feature's property value (or `Null` if absent) on each evaluation.
	referenced_vars: Vec<String>,
	/// The special names the expression reads. `$props` is the expensive one: its map is only
	/// built per feature when it is read.
	specials: Vec<Special>,
	/// Whether the expression calls [`is_num`], which is only registered when it does.
	binds_is_num: bool,
}

/// Hand-written because [`CelContext`] is not `Debug`, and the operations
/// holding an expression need to be.
impl std::fmt::Debug for FeatureExpr {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("FeatureExpr")
			.field("referenced_vars", &self.referenced_vars)
			.field("specials", &self.specials)
			.field("binds_is_num", &self.binds_is_num)
			.finish_non_exhaustive()
	}
}

impl FeatureExpr {
	/// Compiles `expr` and works out which names it reads.
	///
	/// # Errors
	///
	/// When the expression does not compile, nests too deeply to evaluate, or
	/// uses a name it cannot read.
	pub(crate) fn new(expr: &str) -> Result<Self> {
		let program = compile_cel(expr)?;

		let (specials, referenced_vars) = names_read(&program);
		let binds_is_num = program.references().has_function(IS_NUM_FN);

		let mut context = CelContext::default();
		if binds_is_num {
			context.add_function(IS_NUM_FN, is_num);
		}

		Ok(Self {
			program,
			context,
			referenced_vars,
			specials,
			binds_is_num,
		})
	}

	/// How much of a tile the expression's answer depends on: the widest
	/// [`Special::scope`] of the names it reads, or a feature's when it reads a
	/// property.
	///
	/// `$zoom >= 12` reads nothing about the feature, so evaluating it per feature asks the same
	/// question thousands of times — and a zoom range with no predicate is the shape a generated
	/// pipeline emits most. A caller evaluates such an expression once and keeps or drops whole
	/// layers, which also skips decoding and re-encoding their properties. An expression reading
	/// `$layer` as well is answered once per layer, the same way.
	pub(crate) fn scope(&self) -> ExpressionScope {
		scope_of(&self.specials, &self.referenced_vars)
	}

	/// The answer for a whole tile, for an expression whose [`scope`](Self::scope)
	/// is [`ExpressionScope::Tile`].
	pub(crate) fn evaluate_tile(&self, coord: &TileCoord) -> bool {
		self.evaluate(coord, "", None, &GeoProperties::new())
	}

	/// The answer for a whole layer, for an expression whose [`scope`](Self::scope)
	/// is [`ExpressionScope::Tile`] or [`ExpressionScope::Layer`].
	pub(crate) fn evaluate_layer(&self, coord: &TileCoord, layer: &str) -> bool {
		self.evaluate(coord, layer, None, &GeoProperties::new())
	}

	/// Whether `feature`, with its decoded `properties`, in the layer named
	/// `layer` of the tile at `coord`, passes.
	pub(crate) fn evaluate_feature(
		&self,
		coord: &TileCoord,
		layer: &str,
		feature: &VectorTileFeature,
		properties: &GeoProperties,
	) -> bool {
		self.evaluate(coord, layer, Some(feature), properties)
	}

	/// An evaluation error or a result that is not a boolean is `false`. A name
	/// the caller has nothing for — `$type` without a feature — is `null`.
	fn evaluate(
		&self,
		coord: &TileCoord,
		layer: &str,
		feature: Option<&VectorTileFeature>,
		props: &GeoProperties,
	) -> bool {
		// A child of the prepared context: it carries only this feature's variables and resolves
		// functions through its parent, so the builtins are not reinstalled per feature.
		let mut ctx = self.context.new_inner_scope();

		for special in &self.specials {
			let value = match special {
				// `Int` because CEL's integer literals are signed, so `$zoom` and the `12` in
				// `$zoom >= 12` are the same type. `cel-interpreter` compares `UInt` against a
				// signed literal correctly too, so this is a matter of matching the literal rather
				// than a correctness requirement.
				Special::Zoom => CelValue::Int(i64::from(coord.level)),
				Special::X => CelValue::Int(i64::from(coord.x)),
				Special::Y => CelValue::Int(i64::from(coord.y)),
				Special::Layer => CelValue::String(Arc::new(layer.to_string())),
				Special::Type => feature.map_or(CelValue::Null, |feature| geom_type_to_cel(feature.geom_type)),
				// Signed where it fits, like `$zoom`, so `$id == 42` compares like with like.
				Special::Id => match feature.and_then(|feature| feature.id) {
					Some(id) => i64::try_from(id).map_or(CelValue::UInt(id), CelValue::Int),
					None => CelValue::Null,
				},
				Special::Props => {
					let mut map: HashMap<CelKey, CelValue> = HashMap::with_capacity(props.len());
					for (key, value) in props.iter() {
						map.insert(CelKey::from(key.clone()), geo_to_cel(value));
					}
					CelValue::Map(CelMap { map: Arc::new(map) })
				}
			};
			ctx.add_variable_from_value(special.var(), value);
		}

		for name in &self.referenced_vars {
			let value = props.get(name.as_str()).map_or(CelValue::Null, geo_to_cel);
			ctx.add_variable_from_value(name.clone(), value);
		}

		matches!(self.program.execute(&ctx), Ok(CelValue::Bool(true)))
	}
}

/// `$type` as the strings MapLibre's `geometry-type` uses.
///
/// An MVT feature only says point, line or polygon, never whether there is one
/// or several, so these are the singular names, which is what MapLibre reports
/// for a tile's features too. A feature of unknown type is `null`.
fn geom_type_to_cel(geom_type: GeomType) -> CelValue {
	let name = match geom_type {
		GeomType::MultiPoint => "Point",
		GeomType::MultiLineString => "LineString",
		GeomType::MultiPolygon => "Polygon",
		GeomType::Unknown => return CelValue::Null,
	};
	CelValue::String(Arc::new(name.to_string()))
}

fn geo_to_cel(v: &GeoValue) -> CelValue {
	match v {
		GeoValue::Bool(b) => CelValue::Bool(*b),
		GeoValue::Int(i) => CelValue::Int(*i),
		GeoValue::UInt(u) => CelValue::UInt(*u),
		GeoValue::Float(f) => CelValue::Float(f64::from(*f)),
		GeoValue::Double(d) => CelValue::Float(*d),
		GeoValue::String(s) => CelValue::String(Arc::new(s.clone())),
		GeoValue::Null => CelValue::Null,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn compile_error(expr: &str) -> String {
		format!("{:#}", FeatureExpr::new(expr).unwrap_err())
	}

	/// The endings that crash `cel-parser` get a syntax error of their own,
	/// and never reach it, so nothing panics (#325).
	#[test]
	fn an_expression_that_ends_too_early_says_where() {
		for (expr, needle) in [
			("$layer ==", "ends after `==`, which needs something to compare with"),
			("kind !=", "ends after `!=`"),
			("$zoom >", "ends after `>`"),
			("$zoom +", "ends after `+`, which needs a value after it"),
			("kind &&", "ends after `&&`, which needs a second condition"),
			("kind ||", "ends after `||`"),
			("kind in", "ends after `in`, which needs a list or a map to look in"),
			("!", "ends after `!`, which needs a condition to negate"),
			("kind == 'a' &&", "ends after `&&`"),
			("kind ? 1 :", "ends after `:`"),
			("{'a':", "ends after `:`"),
			("kind == 'a", "the string that starts here is never closed"),
			("kind == r'a", "the string that starts here is never closed"),
			("kind == '''a", "the string that starts here is never closed"),
		] {
			let error = compile_error(expr);
			assert!(error.contains(needle), "{expr}: {error}");
			assert!(!error.contains("Parser crashed"), "{expr}: {error}");
		}
	}

	#[test]
	fn the_error_has_the_parser_layout() {
		let error = compile_error("$layer ==");
		assert!(
			error.ends_with(
				"ERROR: <input>:1:10: Syntax error: the expression ends after `==`, which needs something to compare with\n\
				 | $layer ==\n\
				 | .........^"
			),
			"{error}"
		);
		// The caret stands at the opening quote, on the line it is on.
		let error = compile_error("$zoom > 1 &&\n  kind == 'a");
		assert!(error.contains("ERROR: <input>:2:11: "), "{error}");
		assert!(error.ends_with("|   kind == 'a\n| ..........^"), "{error}");
	}

	/// An operator in a string or a comment is not where the expression ends.
	#[test]
	fn operators_in_strings_and_comments_end_nothing() {
		for expr in [
			"kind == 'a =='",
			"kind == \"&&\"",
			"kind == 'x' // ==",
			"kind == '''a ||'''",
			"$zoom > 1 ? 'a' : 'b'",
			"{'a': 1}['a'] == 1",
			"-1 < $zoom",
		] {
			FeatureExpr::new(expr).unwrap_or_else(|e| panic!("{expr}: {e:#}"));
		}
	}

	/// What an editor offers is what compiles: every listed name is read by
	/// an expression, and they are listed in the order of the table.
	#[test]
	fn every_listed_name_compiles() {
		let names: Vec<&str> = expression_names().iter().map(|(name, _)| *name).collect();
		assert_eq!(names, ["$zoom", "$x", "$y", "$layer", "$type", "$id", "$props"]);
		for (name, doc) in expression_names() {
			FeatureExpr::new(&format!("{name} != null")).unwrap_or_else(|e| panic!("{name}: {e:#}"));
			assert!(doc.ends_with('.') && !doc.is_empty(), "{name}: {doc:?}");
		}
	}

	#[test]
	fn is_num_is_only_registered_when_referenced() {
		// Registered per evaluation, so an expression that does not mention it should not pay for
		// it — and an expression that calls an undefined function errors, which is what makes the
		// absence observable at all.
		assert!(!FeatureExpr::new("$props.v == 1").unwrap().binds_is_num);
		assert!(FeatureExpr::new("is_num($props.v)").unwrap().binds_is_num);
	}

	fn rewrite(expr: &str) -> String {
		rewrite_special_names(expr).unwrap()
	}

	fn rewrite_error(expr: &str) -> String {
		format!("{:#}", rewrite_special_names(expr).unwrap_err())
	}

	#[test]
	fn special_names_are_rewritten_in_code() {
		assert_eq!(
			rewrite("$zoom >= 12 && $layer == 'x' && has($props.a) && 'b' in $props"),
			"_zoom >= 12 && _layer == 'x' && has(_props.a) && 'b' in _props"
		);
		assert_eq!(rewrite("($zoom)"), "(_zoom)");
	}

	#[test]
	fn string_literals_are_left_alone_in_every_form() {
		for expr in [
			"name == '$zoom'",
			r#"name == "$zoom""#,
			"name == '''$zoom\n$layer'''",
			"name == \"\"\"it's $zoom\"\"\"",
			r"name == 'it\'s $zoom'",
			r#"name == "a \" $zoom""#,
			"name == b'$zoom'",
			"name == R'$zoom'",
			"name == br'$zoom'",
		] {
			assert_eq!(rewrite(expr), expr, "{expr}");
		}
	}

	#[test]
	fn a_raw_string_ends_at_its_quote_despite_a_backslash() {
		// In a raw string `\` escapes nothing, so `r'\'` is a complete literal and what follows
		// is code again.
		assert_eq!(rewrite(r"name == r'\' || $zoom > 1"), r"name == r'\' || _zoom > 1");
	}

	#[test]
	fn an_empty_string_does_not_swallow_what_follows() {
		assert_eq!(rewrite("name == '' || $zoom > 1"), "name == '' || _zoom > 1");
	}

	#[test]
	fn comments_are_left_alone_and_do_not_open_strings() {
		assert_eq!(
			rewrite("// it's $zoom\n$zoom > 1 // $layer"),
			"// it's $zoom\n_zoom > 1 // $layer"
		);
	}

	#[test]
	fn letters_in_numbers_are_not_names() {
		assert_eq!(
			rewrite("v > 1e5 && w < 1000u && $zoom > 0x1F"),
			"v > 1e5 && w < 1000u && _zoom > 0x1F"
		);
	}

	#[test]
	fn an_unknown_special_name_is_refused() {
		let message = rewrite_error("$zooom > 1");
		assert!(message.contains("`$zooom`"), "{message}");
		assert!(
			message.contains("`$zoom`, `$x`, `$y`, `$layer`, `$type`, `$id` and `$props`"),
			"{message}"
		);
		assert!(rewrite_error("$ > 1").contains("`$`"));
	}

	#[test]
	fn a_special_name_cannot_be_a_field() {
		assert!(rewrite_error("$props.$zoom > 1").contains("cannot follow a `.`"));
	}

	#[test]
	fn the_compiled_names_are_reserved_but_reachable_as_fields() {
		let message = rewrite_error("_zoom > 1");
		assert!(message.contains("`$props._zoom`"), "{message}");
		for reserved in ["_x", "_y", "_type", "_id"] {
			let message = rewrite_error(&format!("{reserved} == 1"));
			assert!(message.contains(&format!("`$props.{reserved}`")), "{message}");
		}
		assert!(
			rewrite_special_names("_layerx == 1").is_ok(),
			"only the exact names are reserved"
		);
		assert_eq!(rewrite("$props._zoom > 1"), "_props._zoom > 1");
		assert_eq!(rewrite("$props . _zoom > 1"), "_props . _zoom > 1");
	}

	#[test]
	fn the_former_bare_names_are_refused_with_their_new_spelling() {
		for (expr, hint) in [
			("zoom >= 12", "`zoom` is written `$zoom` now"),
			("has(props.name)", "`props` is written `$props` now"),
		] {
			let message = format!("{:#}", compile_cel(expr).unwrap_err());
			assert!(message.contains(hint), "{message}");
		}
		assert!(compile_cel("$props.zoom == 1 && $props.props == 2").is_ok());
		assert!(compile_cel("layer == 1").is_ok(), "`layer` was never reserved");
	}

	#[test]
	fn a_parser_error_shows_the_expression_as_written() {
		let message = format!("{:#}", compile_cel("$zoom >= && $layer").unwrap_err());
		assert!(!message.contains("_zoom"), "{message}");
		assert!(!message.contains("_layer"), "{message}");
	}

	#[test]
	fn the_scope_follows_the_names_read() {
		let scope = |expr: &str| FeatureExpr::new(expr).unwrap().scope();
		assert_eq!(scope("$zoom >= 12"), ExpressionScope::Tile);
		assert_eq!(scope("true"), ExpressionScope::Tile);
		assert_eq!(scope("$layer == 'a' && $zoom > 1"), ExpressionScope::Layer);
		assert_eq!(scope("$layer == 'a' && kind == 'b'"), ExpressionScope::Feature);
		assert_eq!(scope("has($props.a)"), ExpressionScope::Feature);
		assert_eq!(scope("$x == 1 && $y < 4 && $zoom > 2"), ExpressionScope::Tile);
		assert_eq!(scope("$layer == 'a' && $x == 1"), ExpressionScope::Layer);
		assert_eq!(scope("$type == 'Point'"), ExpressionScope::Feature);
		assert_eq!(scope("$id == 1"), ExpressionScope::Feature);
	}

	fn passes(expr: &str, feature: &VectorTileFeature) -> bool {
		let coord = TileCoord::new(5, 3, 7).unwrap();
		FeatureExpr::new(expr)
			.unwrap()
			.evaluate_feature(&coord, "poi", feature, &GeoProperties::new())
	}

	#[test]
	fn x_and_y_are_the_tile_column_and_row() {
		let expr = FeatureExpr::new("$x == 3 && $y == 7 && $zoom == 5").unwrap();
		assert!(expr.evaluate_tile(&TileCoord::new(5, 3, 7).unwrap()));
		assert!(!expr.evaluate_tile(&TileCoord::new(5, 7, 3).unwrap()));
	}

	#[test]
	fn type_is_the_maplibre_name_of_the_geometry_type() {
		for (geom_type, name) in [
			(GeomType::MultiPoint, "Point"),
			(GeomType::MultiLineString, "LineString"),
			(GeomType::MultiPolygon, "Polygon"),
		] {
			let feature = VectorTileFeature {
				geom_type,
				..VectorTileFeature::default()
			};
			assert!(passes(&format!("$type == '{name}'"), &feature), "{geom_type:?}");
		}
		assert!(
			passes("$type == null", &VectorTileFeature::default()),
			"unknown is null"
		);
	}

	#[test]
	fn id_is_the_feature_id_or_null() {
		let with_id = |id| VectorTileFeature {
			id: Some(id),
			..VectorTileFeature::default()
		};
		assert!(passes("$id == 42", &with_id(42)), "compares with a signed literal");
		assert!(passes("$id > 41 && $id < 43", &with_id(42)));
		assert!(passes("$id == 18446744073709551615u", &with_id(u64::MAX)), "beyond i64");
		assert!(passes("$id == null", &VectorTileFeature::default()));
	}

	/// `Program::execute` runs per feature on the tiling thread, with none of
	/// the protection the parse gets, and recurses over the same tree. An
	/// expression deep enough to trouble the parser therefore aborted the
	/// process during a conversion rather than failing at build.
	#[test]
	fn a_deeply_nested_expression_is_refused_at_compile_time() {
		let depth = 4_000;
		let expr = format!("{}$zoom > 1{}", "(".repeat(depth), ")".repeat(depth));

		let error = compile_cel(&expr).expect_err("nesting this deep must be refused");
		let message = format!("{error:#}");
		assert!(message.contains("nests brackets"), "unhelpful message: {message}");
	}

	/// The shape `vector_reduce_to_style` generates is a long chain of shallow
	/// terms — `(a) || (b) || (c) …`, tens of kilobytes for a real style. Its
	/// depth is about one however long it gets, and capping length instead of
	/// depth would break it.
	#[test]
	fn a_long_shallow_chain_still_compiles() {
		let expr = (0..5_000)
			.map(|i| format!("($zoom > {i})"))
			.collect::<Vec<_>>()
			.join(" || ");

		assert!(ensure_cel_nesting_is_bounded(&expr).is_ok());
	}

	/// Brackets inside a string literal are data, not nesting — a generated
	/// style embeds property values verbatim.
	#[test]
	fn brackets_inside_string_literals_do_not_count() {
		let expr = format!("name == '{}'", "(".repeat(500));
		assert!(ensure_cel_nesting_is_bounded(&expr).is_ok());

		// Including one escaped just before the closing quote.
		let expr = r"name == 'a\\' && kind == '((((('";
		assert!(ensure_cel_nesting_is_bounded(expr).is_ok());
	}

	/// Ordinary expressions are unaffected, right up to the limit.
	#[test]
	fn ordinary_expressions_are_unaffected() {
		assert!(ensure_cel_nesting_is_bounded("$zoom >= 12").is_ok());
		assert!(ensure_cel_nesting_is_bounded("has($props.name) && $props.population >= 1000").is_ok());

		let at_limit = format!(
			"{}$zoom > 1{}",
			"(".repeat(MAX_CEL_NESTING_DEPTH),
			")".repeat(MAX_CEL_NESTING_DEPTH)
		);
		assert!(
			ensure_cel_nesting_is_bounded(&at_limit).is_ok(),
			"the limit itself must pass"
		);

		let past_limit = format!(
			"{}$zoom > 1{}",
			"(".repeat(MAX_CEL_NESTING_DEPTH + 1),
			")".repeat(MAX_CEL_NESTING_DEPTH + 1)
		);
		assert!(ensure_cel_nesting_is_bounded(&past_limit).is_err());
	}
}
