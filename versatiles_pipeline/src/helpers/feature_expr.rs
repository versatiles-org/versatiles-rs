//! Feature expressions: the CEL predicates operations evaluate per feature.
//!
//! `vector_filter_features` was the only operation that tested features, so
//! this lived inside it, and `vector_reduce_to_style` borrowed its runner. It is
//! shared now because more operations need the same thing: an expression that
//! compiles the same way, reserves the same names and binds them the same way
//! wherever it appears.

use std::{collections::HashMap, sync::Arc};

use anyhow::{Result, anyhow, ensure};
use cel_interpreter::{
	Context as CelContext, Program, Value as CelValue,
	extractors::This,
	objects::{Key as CelKey, Map as CelMap},
};
use versatiles_core::TileCoord;
use versatiles_geometry::geo::{GeoProperties, GeoValue};

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

/// Compiles a CEL expression, containing the parser's panics.
///
/// `cel-parser 0.10.1` panics on some malformed inputs instead of returning
/// `ParseErrors`. Containing the panic keeps a bad expression from taking down
/// the whole pipeline — and now also from taking down an editor asking `check`
/// what is wrong with the line someone is halfway through typing.
///
/// The recovery is silent to the caller but not to the terminal: `catch_unwind`
/// still runs the panic hook, so a panicking expression prints
/// `thread … panicked at antlr4rust …` to stderr. Silencing it would mean
/// installing a process-global panic hook from inside a library, which is the
/// shape of problem #261 was filed about. Whoever owns the process can set a
/// hook; this function will not.
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
fn compile_cel(expr: &str) -> Result<Program> {
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
	ensure_cel_nesting_is_bounded(expr)?;

	// `ParseErrors` holds an `Rc` and so cannot cross a thread boundary; it is rendered to its
	// message here, inside the thread, which is all the caller ever did with it.
	let parse = || {
		std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
			Program::compile(expr).map_err(|e| e.to_string())
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

	compiled
		.map_err(|panic_payload| {
			// `catch_unwind` returns `Box<dyn Any + Send>`; extract a message if the panic
			// carried one (Rust panics with string literals or `format!`-ed Strings).
			let detail = panic_payload
				.downcast_ref::<String>()
				.map(String::as_str)
				.or_else(|| panic_payload.downcast_ref::<&'static str>().copied())
				.unwrap_or("no panic message");
			anyhow!(
				"Failed to compile CEL expression:\n  {expr}\n\n\
				 Parser crashed (likely malformed CEL input): {detail}\n\n\
				 Common causes: unmatched brackets, trailing operators, unsupported tokens. \
				 Run `versatiles help pipeline` for the CEL operator cheat-sheet."
			)
		})?
		.map_err(|e| {
			// `ParseErrors::Display` already renders `ERROR: <input>:L:C: msg` plus a source
			// snippet with a caret. Put the expression and the report on separate lines so
			// the caret alignment survives and terminal output stays readable.
			anyhow!("Failed to compile CEL expression:\n  {expr}\n\n{e}")
		})
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
fn ensure_cel_nesting_is_bounded(expr: &str) -> Result<()> {
	let mut depth = 0usize;
	let mut deepest = 0usize;
	let mut quote: Option<char> = None;
	let mut escaped = false;

	for c in expr.chars() {
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
			}
			')' | ']' | '}' => depth = depth.saturating_sub(1),
			_ => {}
		}
	}

	ensure!(
		deepest <= MAX_CEL_NESTING_DEPTH,
		"CEL expression nests brackets {deepest} deep, more than the {MAX_CEL_NESTING_DEPTH} this evaluates:\n  {expr}"
	);

	Ok(())
}

/// The name bound to the tile's zoom level, reserved like [`PROPS_VAR`].
const ZOOM_VAR: &str = "zoom";

/// The name bound to the feature's full property map.
const PROPS_VAR: &str = "props";

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

/// A compiled feature expression, evaluated against one feature at a time.
///
/// Knows which names the expression reads, so each evaluation binds only those:
/// the reserved `zoom` and `props`, and the feature properties it mentions.
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
	/// Top-level identifiers referenced by the expression, other than the reserved names.
	/// Bound to the feature's property value (or `Null` if absent) on each evaluation.
	referenced_vars: Vec<String>,
	/// Whether the expression references the reserved `props` map. The map is only built
	/// per-feature when needed.
	binds_props: bool,
	/// Whether the expression references the reserved `zoom` variable.
	binds_zoom: bool,
	/// Whether the expression calls [`is_num`], which is only registered when it does.
	binds_is_num: bool,
}

/// Hand-written because [`CelContext`] is not `Debug`, and the operations
/// holding an expression need to be.
impl std::fmt::Debug for FeatureExpr {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("FeatureExpr")
			.field("referenced_vars", &self.referenced_vars)
			.field("binds_props", &self.binds_props)
			.field("binds_zoom", &self.binds_zoom)
			.field("binds_is_num", &self.binds_is_num)
			.finish_non_exhaustive()
	}
}

impl FeatureExpr {
	/// Compiles `expr` and works out which names it reads.
	///
	/// # Errors
	///
	/// When the expression does not compile, or nests too deeply to evaluate.
	pub(crate) fn new(expr: &str) -> Result<Self> {
		let program = compile_cel(expr)?;

		let refs = program.references();
		let binds_props = refs.has_variable(PROPS_VAR);
		let binds_zoom = refs.has_variable(ZOOM_VAR);
		let binds_is_num = refs.has_function(IS_NUM_FN);
		let referenced_vars: Vec<String> = refs
			.variables()
			.into_iter()
			.filter(|v| *v != PROPS_VAR && *v != ZOOM_VAR)
			.map(String::from)
			.collect();

		let mut context = CelContext::default();
		if binds_is_num {
			context.add_function(IS_NUM_FN, is_num);
		}

		Ok(Self {
			program,
			context,
			referenced_vars,
			binds_props,
			binds_zoom,
			binds_is_num,
		})
	}

	/// Whether the expression's answer is the same for every feature in a tile.
	///
	/// `zoom >= 12` reads nothing about the feature, so evaluating it per feature asks the same
	/// question thousands of times — and a zoom range with no predicate is the shape a generated
	/// pipeline emits most. A caller evaluates such an expression once and keeps or drops whole
	/// layers, which also skips decoding and re-encoding their properties.
	pub(crate) fn depends_only_on_zoom(&self) -> bool {
		self.binds_zoom && !self.binds_props && self.referenced_vars.is_empty()
	}

	/// Whether a feature with `props`, in the tile at `coord`, passes.
	///
	/// An evaluation error or a result that is not a boolean is `false`.
	pub(crate) fn evaluate(&self, coord: &TileCoord, props: &GeoProperties) -> bool {
		// A child of the prepared context: it carries only this feature's variables and resolves
		// functions through its parent, so the builtins are not reinstalled per feature.
		let mut ctx = self.context.new_inner_scope();

		if self.binds_zoom {
			// `Int` because CEL's integer literals are signed, so `zoom` and the `12` in
			// `zoom >= 12` are the same type. `cel-interpreter` compares `UInt` against a signed
			// literal correctly too, so this is a matter of matching the literal rather than a
			// correctness requirement.
			ctx.add_variable_from_value(ZOOM_VAR, CelValue::Int(i64::from(coord.level)));
		}

		if self.binds_props {
			let mut map: HashMap<CelKey, CelValue> = HashMap::with_capacity(props.len());
			for (key, value) in props.iter() {
				map.insert(CelKey::from(key.clone()), geo_to_cel(value));
			}
			ctx.add_variable_from_value(PROPS_VAR, CelValue::Map(CelMap { map: Arc::new(map) }));
		}

		for name in &self.referenced_vars {
			let value = props.get(name.as_str()).map_or(CelValue::Null, geo_to_cel);
			ctx.add_variable_from_value(name.clone(), value);
		}

		matches!(self.program.execute(&ctx), Ok(CelValue::Bool(true)))
	}
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

	#[test]
	fn is_num_is_only_registered_when_referenced() {
		// Registered per evaluation, so an expression that does not mention it should not pay for
		// it — and an expression that calls an undefined function errors, which is what makes the
		// absence observable at all.
		assert!(!FeatureExpr::new("props.v == 1").unwrap().binds_is_num);
		assert!(FeatureExpr::new("is_num(props.v)").unwrap().binds_is_num);
	}

	/// `Program::execute` runs per feature on the tiling thread, with none of
	/// the protection the parse gets, and recurses over the same tree. An
	/// expression deep enough to trouble the parser therefore aborted the
	/// process during a conversion rather than failing at build.
	#[test]
	fn a_deeply_nested_expression_is_refused_at_compile_time() {
		let depth = 4_000;
		let expr = format!("{}zoom > 1{}", "(".repeat(depth), ")".repeat(depth));

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
			.map(|i| format!("(zoom > {i})"))
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
		assert!(ensure_cel_nesting_is_bounded("zoom >= 12").is_ok());
		assert!(ensure_cel_nesting_is_bounded("has(props.name) && props.population >= 1000").is_ok());

		let at_limit = format!(
			"{}zoom > 1{}",
			"(".repeat(MAX_CEL_NESTING_DEPTH),
			")".repeat(MAX_CEL_NESTING_DEPTH)
		);
		assert!(
			ensure_cel_nesting_is_bounded(&at_limit).is_ok(),
			"the limit itself must pass"
		);

		let past_limit = format!(
			"{}zoom > 1{}",
			"(".repeat(MAX_CEL_NESTING_DEPTH + 1),
			")".repeat(MAX_CEL_NESTING_DEPTH + 1)
		);
		assert!(ensure_cel_nesting_is_bounded(&past_limit).is_err());
	}
}
