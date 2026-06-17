//! Issue model, aggregation registry, and output formatting for the shortbread
//! conformance check.
//!
//! A full-planet scan touches millions of features, so individual findings are
//! never logged. Instead every check produces a compact [`Issue`] that is folded
//! into a [`Registry`] keyed by `(severity, rule, layer, attr, value)`, keeping
//! only a running count and a few example tile coordinates.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use versatiles_core::TileCoord;

/// How many example tile coordinates to keep per distinct issue.
const MAX_SAMPLES: usize = 3;

/// Reporting severity. Ordered `Hint < Warn < Error` so issues sort and filter
/// naturally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
pub enum Severity {
	Hint,
	Warn,
	Error,
}

impl Severity {
	/// Under `--strict`, soft findings are escalated one level: `Hint → Warn`,
	/// `Warn → Error`. `Error` stays `Error`.
	pub fn promote(self, strict: bool) -> Severity {
		if !strict {
			return self;
		}
		match self {
			Severity::Hint => Severity::Warn,
			Severity::Warn | Severity::Error => Severity::Error,
		}
	}

	pub fn label(self) -> &'static str {
		match self {
			Severity::Error => "error",
			Severity::Warn => "warn",
			Severity::Hint => "hint",
		}
	}

	fn heading(self) -> &'static str {
		match self {
			Severity::Error => "ERRORS",
			Severity::Warn => "WARNINGS",
			Severity::Hint => "HINTS",
		}
	}
}

/// The kind of conformance problem found. The display message is templated from
/// the issue's `layer`/`attr`/`value` fields at render time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
	UnknownLayer,
	WrongGeometry,
	BelowMinzoom,
	BadExtent,
	UnknownAttribute,
	MissingRequired,
	WrongType,
	BadEnumValue,
}

impl Rule {
	pub fn id(self) -> &'static str {
		match self {
			Rule::UnknownLayer => "unknown_layer",
			Rule::WrongGeometry => "wrong_geometry",
			Rule::BelowMinzoom => "below_minzoom",
			Rule::BadExtent => "bad_extent",
			Rule::UnknownAttribute => "unknown_attribute",
			Rule::MissingRequired => "missing_required",
			Rule::WrongType => "wrong_type",
			Rule::BadEnumValue => "bad_enum_value",
		}
	}
}

/// A single finding from one tile, before aggregation.
#[derive(Clone, Debug)]
pub struct Issue {
	pub severity: Severity,
	pub rule: Rule,
	pub layer: String,
	pub attr: Option<String>,
	pub value: Option<String>,
	/// Free-form detail rendered after the templated message (e.g. the observed
	/// vs expected geometry/type).
	pub detail: Option<String>,
	pub coord: TileCoord,
}

impl Issue {
	pub fn new(severity: Severity, rule: Rule, layer: &str, coord: TileCoord) -> Issue {
		Issue {
			severity,
			rule,
			layer: layer.to_owned(),
			attr: None,
			value: None,
			detail: None,
			coord,
		}
	}

	pub fn attr(mut self, attr: &str) -> Issue {
		self.attr = Some(attr.to_owned());
		self
	}

	pub fn value(mut self, value: &str) -> Issue {
		self.value = Some(value.to_owned());
		self
	}

	pub fn detail(mut self, detail: String) -> Issue {
		self.detail = Some(detail);
		self
	}
}

/// Aggregation key: everything but the example coordinate.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
	rule: Rule,
	layer: String,
	attr: Option<String>,
	value: Option<String>,
}

struct Agg {
	severity: Severity,
	detail: Option<String>,
	count: u64,
	samples: Vec<TileCoord>,
}

/// Folds per-tile [`Issue`]s into counted, sampled aggregates.
#[derive(Default)]
pub struct Registry {
	entries: BTreeMap<Key, Agg>,
}

impl Registry {
	pub fn new() -> Registry {
		Registry::default()
	}

	/// Records one issue, incrementing its aggregate and keeping up to
	/// [`MAX_SAMPLES`] example coordinates.
	pub fn record(&mut self, issue: Issue) {
		let key = Key {
			rule: issue.rule,
			layer: issue.layer,
			attr: issue.attr,
			value: issue.value,
		};
		let agg = self.entries.entry(key).or_insert_with(|| Agg {
			severity: issue.severity,
			detail: issue.detail.clone(),
			count: 0,
			samples: Vec::new(),
		});
		agg.count += 1;
		// Keep up to MAX_SAMPLES *distinct* example tiles — a single tile with
		// many offending features should not fill every sample slot.
		if agg.samples.len() < MAX_SAMPLES && !agg.samples.contains(&issue.coord) {
			agg.samples.push(issue.coord);
		}
	}

	pub fn merge(&mut self, issues: Vec<Issue>) {
		for issue in issues {
			self.record(issue);
		}
	}

	/// Number of distinct findings at or above `threshold` after `strict`
	/// promotion. Drives the exit code.
	pub fn count_at_or_above(&self, threshold: Severity, strict: bool) -> usize {
		self
			.entries
			.values()
			.filter(|a| a.severity.promote(strict) >= threshold)
			.count()
	}

	/// Distinct-finding counts as `(errors, warnings, hints)` after `strict`
	/// promotion, for the summary footer line.
	pub fn histogram(&self, strict: bool) -> (usize, usize, usize) {
		let (mut e, mut w, mut h) = (0, 0, 0);
		for agg in self.entries.values() {
			match agg.severity.promote(strict) {
				Severity::Error => e += 1,
				Severity::Warn => w += 1,
				Severity::Hint => h += 1,
			}
		}
		(e, w, h)
	}

	/// Rows at or above `min_severity`, sorted by severity desc then count desc.
	fn visible_rows(&self, min_severity: Severity, strict: bool) -> Vec<(&Key, &Agg, Severity)> {
		let mut rows: Vec<(&Key, &Agg, Severity)> = self
			.entries
			.iter()
			.map(|(k, a)| (k, a, a.severity.promote(strict)))
			.filter(|(_, _, sev)| *sev >= min_severity)
			.collect();
		rows.sort_by(|(ka, aa, sa), (kb, ab, sb)| sb.cmp(sa).then(ab.count.cmp(&aa.count)).then(ka.cmp(kb)));
		rows
	}
}

fn coord_str(c: TileCoord) -> String {
	format!("{}/{}/{}", c.level, c.x, c.y)
}

fn samples_str(samples: &[TileCoord]) -> String {
	let joined = samples.iter().map(|c| coord_str(*c)).collect::<Vec<_>>().join(", ");
	if joined.is_empty() {
		joined
	} else {
		format!("e.g. {joined}")
	}
}

/// One-line human description of an aggregated finding.
fn message(key: &Key, agg: &Agg) -> String {
	let what = match key.rule {
		Rule::UnknownLayer => "unknown layer".to_owned(),
		Rule::WrongGeometry => "wrong geometry".to_owned(),
		Rule::BelowMinzoom => "appears below its minzoom".to_owned(),
		Rule::BadExtent => "non-standard extent".to_owned(),
		Rule::UnknownAttribute => format!("unknown attribute `{}`", key.attr.as_deref().unwrap_or("?")),
		Rule::MissingRequired => format!("missing required attribute `{}`", key.attr.as_deref().unwrap_or("?")),
		Rule::WrongType => format!("wrong type for `{}`", key.attr.as_deref().unwrap_or("?")),
		Rule::BadEnumValue => format!(
			"`{}` = {:?} not in enum",
			key.attr.as_deref().unwrap_or("?"),
			key.value.as_deref().unwrap_or("?")
		),
	};
	match &agg.detail {
		Some(d) => format!("{what} ({d})"),
		None => what,
	}
}

/// Renders a grouped summary: counts per severity, each row with examples.
pub fn render_summary(reg: &Registry, version: &str, min_severity: Severity, strict: bool) -> String {
	let mut out = String::new();
	let _ = writeln!(
		out,
		"shortbread {version} check{}\n",
		if strict { " (strict)" } else { "" }
	);

	let rows = reg.visible_rows(min_severity, strict);
	if rows.is_empty() {
		let _ = writeln!(out, "no issues at or above `{}`.", min_severity.label());
		return out;
	}

	let mut current: Option<Severity> = None;
	for (key, agg, sev) in &rows {
		if current != Some(*sev) {
			let n = rows.iter().filter(|(_, _, s)| s == sev).count();
			let _ = writeln!(
				out,
				"{}  ({n} {})",
				sev.heading(),
				if n == 1 { "rule" } else { "rules" }
			);
			current = Some(*sev);
		}
		let _ = writeln!(
			out,
			"  {:<22} {:<44} {:>8}×   {}",
			key.layer,
			message(key, agg),
			agg.count,
			samples_str(&agg.samples)
		);
	}
	out
}

/// Renders every distinct finding, one per line.
pub fn render_list(reg: &Registry, version: &str, min_severity: Severity, strict: bool) -> String {
	let mut out = String::new();
	let _ = writeln!(
		out,
		"shortbread {version} check{}\n",
		if strict { " (strict)" } else { "" }
	);
	for (key, agg, sev) in reg.visible_rows(min_severity, strict) {
		let _ = writeln!(
			out,
			"{:<5} {:<22} {:<44} {:>8}×   {}",
			sev.label(),
			key.layer,
			message(key, agg),
			agg.count,
			samples_str(&agg.samples)
		);
	}
	out
}

fn json_escape(s: &str) -> String {
	let mut o = String::with_capacity(s.len() + 2);
	for c in s.chars() {
		match c {
			'"' => o.push_str("\\\""),
			'\\' => o.push_str("\\\\"),
			'\n' => o.push_str("\\n"),
			'\t' => o.push_str("\\t"),
			'\r' => o.push_str("\\r"),
			c if (c as u32) < 0x20 => {
				let _ = write!(o, "\\u{:04x}", c as u32);
			}
			c => o.push(c),
		}
	}
	o
}

fn json_field_opt(out: &mut String, name: &str, value: Option<&str>) {
	match value {
		Some(v) => {
			let _ = write!(out, "\"{name}\":\"{}\"", json_escape(v));
		}
		None => {
			let _ = write!(out, "\"{name}\":null");
		}
	}
}

/// Renders the visible findings as a JSON array (one object per distinct issue).
pub fn render_json(reg: &Registry, version: &str, min_severity: Severity, strict: bool) -> String {
	let rows = reg.visible_rows(min_severity, strict);
	let mut out = String::new();
	let _ = write!(
		out,
		"{{\"version\":\"{}\",\"strict\":{strict},\"issues\":[",
		json_escape(version)
	);
	for (i, (key, agg, sev)) in rows.iter().enumerate() {
		if i > 0 {
			out.push(',');
		}
		let _ = write!(
			out,
			"{{\"severity\":\"{}\",\"rule\":\"{}\",\"layer\":\"{}\",",
			sev.label(),
			key.rule.id(),
			json_escape(&key.layer)
		);
		json_field_opt(&mut out, "attr", key.attr.as_deref());
		out.push(',');
		json_field_opt(&mut out, "value", key.value.as_deref());
		out.push(',');
		json_field_opt(&mut out, "detail", agg.detail.as_deref());
		let samples = agg
			.samples
			.iter()
			.map(|c| format!("\"{}\"", coord_str(*c)))
			.collect::<Vec<_>>()
			.join(",");
		let _ = write!(out, ",\"count\":{},\"samples\":[{samples}]}}", agg.count);
	}
	out.push_str("]}");
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	fn sample_registry() -> Registry {
		let mut reg = Registry::new();
		for x in 0..5 {
			reg.record(Issue::new(Severity::Error, Rule::MissingRequired, "water_polygons", coord(8, x, 1)).attr("kind"));
		}
		reg.record(Issue::new(
			Severity::Warn,
			Rule::UnknownLayer,
			"extra_layer",
			coord(14, 2, 3),
		));
		reg.record(
			Issue::new(Severity::Hint, Rule::BadEnumValue, "land", coord(11, 1, 1))
				.attr("kind")
				.value("wood"),
		);
		reg
	}

	#[test]
	fn samples_capped_at_three() {
		let reg = sample_registry();
		let rows = reg.visible_rows(Severity::Hint, false);
		let missing = rows.iter().find(|(k, _, _)| k.rule == Rule::MissingRequired).unwrap();
		assert_eq!(missing.1.count, 5);
		assert_eq!(missing.1.samples.len(), 3, "samples must be capped");
	}

	#[test]
	fn min_severity_filters() {
		let reg = sample_registry();
		assert_eq!(reg.visible_rows(Severity::Error, false).len(), 1);
		assert_eq!(reg.visible_rows(Severity::Warn, false).len(), 2);
		assert_eq!(reg.visible_rows(Severity::Hint, false).len(), 3);
	}

	#[test]
	fn strict_promotes_severity() {
		let reg = sample_registry();
		// Without strict, only the error row is >= Error.
		assert_eq!(reg.count_at_or_above(Severity::Error, false), 1);
		// With strict, the warn (→error) and hint (→warn) shift up: 2 rows >= Error.
		assert_eq!(reg.count_at_or_above(Severity::Error, true), 2);
	}

	#[test]
	fn summary_groups_by_severity() {
		let reg = sample_registry();
		let s = render_summary(&reg, "1.1", Severity::Hint, false);
		assert!(s.contains("ERRORS"));
		assert!(s.contains("WARNINGS"));
		assert!(s.contains("HINTS"));
		assert!(s.contains("water_polygons"));
		assert!(s.contains("missing required attribute `kind`"));
	}

	#[test]
	fn json_is_wellformed_and_parses() {
		let reg = sample_registry();
		let j = render_json(&reg, "1.1", Severity::Hint, false);
		let parsed = versatiles_core::json::parse_json_str(&j).expect("valid JSON");
		assert!(j.contains("\"rule\":\"missing_required\""));
		assert!(j.contains("\"value\":\"wood\""));
		drop(parsed);
	}
}
