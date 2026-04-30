use napi_derive::napi;
use std::fmt::Write;
use versatiles::pipeline::vpl::VPLFieldMeta;
use versatiles::pipeline::{OperationMeta, all_operation_metadata};

/// Generate the VPL TypeScript builder source code from Rust operation metadata.
///
/// Returns the complete TypeScript source as a string. Write it to a `.ts` file
/// and compile with `tsc` to produce `.js` + `.d.ts`.
#[napi]
#[allow(unused)]
pub fn generate_vpl_typescript() -> String {
	let ops = all_operation_metadata();
	generate_typescript(&ops)
}

/// Convert a snake_case string to camelCase.
fn to_camel_case(s: &str) -> String {
	let mut result = String::new();
	let mut capitalize_next = false;
	for c in s.chars() {
		if c == '_' {
			capitalize_next = true;
		} else if capitalize_next {
			result.extend(c.to_uppercase());
			capitalize_next = false;
		} else {
			result.push(c);
		}
	}
	result
}

/// Convert a snake_case string to PascalCase.
fn to_pascal_case(s: &str) -> String {
	let mut result = String::new();
	let mut capitalize_next = true;
	for c in s.chars() {
		if c == '_' {
			capitalize_next = true;
		} else if capitalize_next {
			result.extend(c.to_uppercase());
			capitalize_next = false;
		} else {
			result.push(c);
		}
	}
	result
}

/// Map a VPL field's Rust type to a TypeScript type expression.
///
/// Enum-typed fields (carrying a non-empty `enum_variants`) become a TS
/// string-literal union (`"a" | "b" | "c"`). Everything else falls back to
/// the static type-string match below.
fn rust_type_to_ts(field: &VPLFieldMeta) -> String {
	if !field.enum_variants.is_empty() {
		return field
			.enum_variants
			.iter()
			.map(|v| format!(r#""{v}""#))
			.collect::<Vec<_>>()
			.join(" | ");
	}
	let ts: &'static str = match field.rust_type.as_str() {
		"String" | "Option<String>" => "string",
		"bool" | "Option<bool>" => "boolean",
		"u8" | "u16" | "u32" | "f32" | "f64" | "Option<u8>" | "Option<u16>" | "Option<u32>" | "Option<f32>"
		| "Option<f64>" => "number",
		"[f64;4]" | "Option<[f64;4]>" => "[number, number, number, number]",
		"[f64;3]" | "Option<[f64;3]>" | "[u8;3]" | "Option<[u8;3]>" => "[number, number, number]",
		"Vec<VPLPipeline>" => "VPL[]",
		_ => "unknown",
	};
	ts.to_string()
}

/// Check if an operation has a `sources` field.
fn has_sources(op: &OperationMeta) -> bool {
	op.fields.iter().any(|f| f.is_sources)
}

/// Get non-sources fields.
fn non_source_fields(op: &OperationMeta) -> Vec<&VPLFieldMeta> {
	op.fields.iter().filter(|f| !f.is_sources).collect()
}

/// Check if all non-source fields are optional.
fn all_fields_optional(op: &OperationMeta) -> bool {
	non_source_fields(op).iter().all(|f| !f.is_required)
}

/// Generate the complete TypeScript source for the VPL builder.
fn generate_typescript(ops: &[OperationMeta]) -> String {
	let mut out = String::new();

	// Header
	out.push_str("// AUTO-GENERATED — DO NOT EDIT\n");
	out.push_str("// Generated from Rust VPL operation metadata\n\n");
	out.push_str("import { TileSource } from './index.js';\n\n");

	// Serialization helper (module-private)
	generate_serialize_helper(&mut out);

	// Generate interfaces for all operations
	for op in ops {
		generate_interface(&mut out, op);
	}

	// Generate VPL class
	generate_vpl_class(&mut out, ops);

	out
}

/// Generate the serializeParam helper function.
fn generate_serialize_helper(out: &mut String) {
	out.push_str("function serializeParam(key: string, value: unknown): string {\n");
	out.push_str("\tif (typeof value === 'boolean') {\n");
	out.push_str("\t\treturn `${key}=${value}`;\n");
	out.push_str("\t}\n");
	out.push_str("\tif (typeof value === 'number') {\n");
	out.push_str("\t\treturn `${key}=${value}`;\n");
	out.push_str("\t}\n");
	out.push_str("\tif (Array.isArray(value)) {\n");
	out.push_str("\t\treturn `${key}=[${value.join(',')}]`;\n");
	out.push_str("\t}\n");
	out.push_str("\tconst str = String(value);\n");
	out.push_str("\tif (/^[a-zA-Z0-9._-]+$/.test(str)) {\n");
	out.push_str("\t\treturn `${key}=${str}`;\n");
	out.push_str("\t}\n");
	out.push_str("\treturn `${key}=\"${str.replace(/\\\\/g, '\\\\\\\\').replace(/\"/g, '\\\\\"')}\"`;\n");
	out.push_str("}\n\n");
}

/// Generate a TypeScript interface for an operation's options.
fn generate_interface(out: &mut String, op: &OperationMeta) {
	let fields = non_source_fields(op);

	if fields.is_empty() {
		return;
	}

	let interface_name = format!("{}Options", to_pascal_case(&op.tag_name));
	writeln!(out, "export interface {interface_name} {{").expect("writing to string never fails");

	for field in &fields {
		let ts_type = rust_type_to_ts(field);
		let camel_name = to_camel_case(&field.name);
		let optional = if field.is_required { "" } else { "?" };

		if !field.doc.is_empty() {
			writeln!(out, "\t/** {} */", field.doc.replace("*/", "* /")).expect("writing to string never fails");
		}

		writeln!(out, "\t{camel_name}{optional}: {ts_type};").expect("writing to string never fails");
	}

	out.push_str("}\n\n");
}

/// Generate the VPL class.
fn generate_vpl_class(out: &mut String, ops: &[OperationMeta]) {
	out.push_str("interface Step {\n");
	out.push_str("\tname: string;\n");
	out.push_str("\tparams: Record<string, unknown>;\n");
	out.push_str("\tsources?: VPL[];\n");
	out.push_str("}\n\n");

	out.push_str("export class VPL {\n");
	out.push_str("\tprivate steps: Step[];\n\n");

	out.push_str("\tprivate constructor(steps: Step[]) {\n");
	out.push_str("\t\tthis.steps = steps;\n");
	out.push_str("\t}\n\n");

	// Static read methods
	for op in ops.iter().filter(|o| o.kind == "read") {
		generate_read_method(out, op);
	}

	// Instance transform methods
	for op in ops.iter().filter(|o| o.kind == "transform") {
		generate_transform_method(out, op);
	}

	// toString
	generate_to_string_method(out);

	// toJSON
	generate_to_json_method(out);

	// open
	out.push_str("\t/** Execute this VPL pipeline and return a TileSource. */\n");
	out.push_str("\tasync fromPath(dir?: string): Promise<TileSource> {\n");
	out.push_str("\t\treturn TileSource.fromPipeline(JSON.stringify(this.toJSON()), dir);\n");
	out.push_str("\t}\n");

	out.push_str("}\n");
}

/// Generate a static read method on the VPL class.
fn generate_read_method(out: &mut String, op: &OperationMeta) {
	let method_name = to_camel_case(&op.tag_name);
	let interface_name = format!("{}Options", to_pascal_case(&op.tag_name));
	let has_src = has_sources(op);
	let fields = non_source_fields(op);
	let has_opts = !fields.is_empty();
	let all_optional = all_fields_optional(op);
	let tag = &op.tag_name;

	// JSDoc
	let doc_first_line = op.doc.lines().next().unwrap_or("").trim();
	if !doc_first_line.is_empty() {
		writeln!(out, "\t/** {} */", doc_first_line.replace("*/", "* /")).expect("writing to string never fails");
	}

	// Method signature
	match (has_src, has_opts) {
		(true, true) => {
			writeln!(
				out,
				"\tstatic {method_name}(sources: VPL[], options?: Omit<{interface_name}, 'sources'>): VPL {{"
			)
			.expect("writing to string never fails");
		}
		(true, false) => {
			writeln!(out, "\tstatic {method_name}(sources: VPL[]): VPL {{").expect("writing to string never fails");
		}
		(false, true) if all_optional => {
			writeln!(out, "\tstatic {method_name}(options?: {interface_name}): VPL {{")
				.expect("writing to string never fails");
		}
		(false, true) => {
			writeln!(out, "\tstatic {method_name}(options: {interface_name}): VPL {{")
				.expect("writing to string never fails");
		}
		(false, false) => {
			writeln!(out, "\tstatic {method_name}(): VPL {{").expect("writing to string never fails");
		}
	}

	// Method body
	if has_opts {
		out.push_str("\t\tconst params: Record<string, unknown> = {};\n");
		let use_optional_chaining = has_src || all_optional;
		for field in &fields {
			let camel = to_camel_case(&field.name);
			let snake = &field.name;
			let accessor = if use_optional_chaining { "?" } else { "" };
			if field.is_required && !use_optional_chaining {
				writeln!(out, "\t\tparams['{snake}'] = options.{camel};").expect("writing to string never fails");
			} else {
				writeln!(
					out,
					"\t\tif (options{accessor}.{camel} !== undefined) params['{snake}'] = options{accessor}.{camel};"
				)
				.expect("writing to string never fails");
			}
		}
	} else {
		out.push_str("\t\tconst params: Record<string, unknown> = {};\n");
	}

	if has_src {
		writeln!(out, "\t\treturn new VPL([{{ name: '{tag}', params, sources }}]);")
			.expect("writing to string never fails");
	} else {
		writeln!(out, "\t\treturn new VPL([{{ name: '{tag}', params }}]);").expect("writing to string never fails");
	}

	out.push_str("\t}\n\n");
}

/// Generate an instance transform method on the VPL class.
fn generate_transform_method(out: &mut String, op: &OperationMeta) {
	let method_name = to_camel_case(&op.tag_name);
	let interface_name = format!("{}Options", to_pascal_case(&op.tag_name));
	let fields = non_source_fields(op);
	let has_opts = !fields.is_empty();
	let all_optional = all_fields_optional(op);
	let tag = &op.tag_name;

	// JSDoc
	let doc_first_line = op.doc.lines().next().unwrap_or("").trim();
	if !doc_first_line.is_empty() {
		writeln!(out, "\t/** {} */", doc_first_line.replace("*/", "* /")).expect("writing to string never fails");
	}

	// Method signature
	if has_opts && all_optional {
		writeln!(out, "\t{method_name}(options?: {interface_name}): VPL {{").expect("writing to string never fails");
	} else if has_opts {
		writeln!(out, "\t{method_name}(options: {interface_name}): VPL {{").expect("writing to string never fails");
	} else {
		writeln!(out, "\t{method_name}(): VPL {{").expect("writing to string never fails");
	}

	// Method body
	if has_opts {
		out.push_str("\t\tconst params: Record<string, unknown> = {};\n");
		for field in &fields {
			let camel = to_camel_case(&field.name);
			let snake = &field.name;
			if field.is_required && !all_optional {
				writeln!(out, "\t\tparams['{snake}'] = options.{camel};").expect("writing to string never fails");
			} else {
				let accessor = if all_optional { "?" } else { "" };
				writeln!(
					out,
					"\t\tif (options{accessor}.{camel} !== undefined) params['{snake}'] = options{accessor}.{camel};"
				)
				.expect("writing to string never fails");
			}
		}
	} else {
		out.push_str("\t\tconst params: Record<string, unknown> = {};\n");
	}

	writeln!(out, "\t\treturn new VPL([...this.steps, {{ name: '{tag}', params }}]);")
		.expect("writing to string never fails");

	out.push_str("\t}\n\n");
}

/// Generate the toString method for VPL serialization.
fn generate_to_string_method(out: &mut String) {
	out.push_str("\t/** Serialize this VPL pipeline to a string. */\n");
	out.push_str("\ttoString(): string {\n");
	out.push_str("\t\treturn this.steps.map(step => {\n");
	out.push_str("\t\t\tlet result = step.name;\n\n");

	// Handle sources
	out.push_str("\t\t\tif (step.sources && step.sources.length > 0) {\n");
	out.push_str("\t\t\t\tconst pipelines = step.sources.map(s => s.toString());\n");
	out.push_str("\t\t\t\tresult += ' [ ' + pipelines.join(', ') + ' ]';\n");
	out.push_str("\t\t\t}\n\n");

	// Handle params
	out.push_str("\t\t\tfor (const [key, value] of Object.entries(step.params)) {\n");
	out.push_str("\t\t\t\tresult += ' ' + serializeParam(key, value);\n");
	out.push_str("\t\t\t}\n\n");

	out.push_str("\t\t\treturn result;\n");
	out.push_str("\t\t}).join(' | ');\n");
	out.push_str("\t}\n\n");
}

/// Generate the toJSON method for structured pipeline serialization.
fn generate_to_json_method(out: &mut String) {
	out.push_str("\t/** Convert this VPL pipeline to a JSON-serializable array of steps. */\n");
	out.push_str("\ttoJSON(): object[] {\n");
	out.push_str("\t\treturn this.steps.map(step => {\n");
	out.push_str("\t\t\tconst obj: Record<string, unknown> = { name: step.name, params: step.params };\n");
	out.push_str("\t\t\tif (step.sources && step.sources.length > 0) {\n");
	out.push_str("\t\t\t\tobj.sources = step.sources.map(s => s.toJSON());\n");
	out.push_str("\t\t\t}\n");
	out.push_str("\t\t\treturn obj;\n");
	out.push_str("\t\t});\n");
	out.push_str("\t}\n\n");
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_to_camel_case() {
		assert_eq!(to_camel_case("from_container"), "fromContainer");
		assert_eq!(to_camel_case("raster_format"), "rasterFormat");
		assert_eq!(to_camel_case("level_min"), "levelMin");
		assert_eq!(to_camel_case("filename"), "filename");
	}

	#[test]
	fn test_to_pascal_case() {
		assert_eq!(to_pascal_case("from_container"), "FromContainer");
		assert_eq!(to_pascal_case("raster_format"), "RasterFormat");
	}

	fn plain_field(rust_type: &str) -> VPLFieldMeta {
		VPLFieldMeta {
			name: "x".to_string(),
			rust_type: rust_type.to_string(),
			is_required: false,
			is_sources: false,
			doc: String::new(),
			enum_variants: Vec::new(),
		}
	}

	fn enum_field(rust_type: &str, variants: Vec<&'static str>) -> VPLFieldMeta {
		VPLFieldMeta {
			enum_variants: variants,
			..plain_field(rust_type)
		}
	}

	#[test]
	fn test_rust_type_to_ts_basic_types() {
		assert_eq!(rust_type_to_ts(&plain_field("String")), "string");
		assert_eq!(rust_type_to_ts(&plain_field("bool")), "boolean");
		assert_eq!(rust_type_to_ts(&plain_field("u8")), "number");
		assert_eq!(rust_type_to_ts(&plain_field("Option<u8>")), "number");
		assert_eq!(rust_type_to_ts(&plain_field("Option<String>")), "string");
		assert_eq!(rust_type_to_ts(&plain_field("[f64;4]")), "[number, number, number, number]");
		assert_eq!(rust_type_to_ts(&plain_field("Option<[f64;3]>")), "[number, number, number]");
		assert_eq!(rust_type_to_ts(&plain_field("Vec<VPLPipeline>")), "VPL[]");
	}

	#[test]
	fn test_rust_type_to_ts_emits_string_literal_union_for_enums() {
		// Enum-typed fields become a TS string-literal union driven by
		// `enum_variants` — not the `rust_type` string.
		let f = enum_field("Option<TileCompression>", vec!["none", "gzip", "brotli", "zstd"]);
		assert_eq!(rust_type_to_ts(&f), r#""none" | "gzip" | "brotli" | "zstd""#);
	}

	#[test]
	fn test_generate_typescript_read_op() {
		let ops = vec![OperationMeta {
			tag_name: "from_container".to_string(),
			kind: "read",
			doc: "Reads a tile container.".to_string(),
			fields: vec![VPLFieldMeta {
				name: "filename".to_string(),
				rust_type: "String".to_string(),
				is_required: true,
				is_sources: false,
				doc: "The filename of the tile container.".to_string(),
				enum_variants: Vec::new(),
			}],
		}];

		let ts = generate_typescript(&ops);
		assert!(ts.contains("AUTO-GENERATED"));
		assert!(ts.contains("export interface FromContainerOptions"));
		assert!(ts.contains("filename: string;"));
		assert!(ts.contains("static fromContainer(options: FromContainerOptions): VPL"));
		assert!(ts.contains("params['filename'] = options.filename;"));
		assert!(ts.contains("function serializeParam"));
	}

	#[test]
	fn test_generate_typescript_transform_op() {
		let ops = vec![OperationMeta {
			tag_name: "filter".to_string(),
			kind: "transform",
			doc: "Filter tiles.".to_string(),
			fields: vec![
				VPLFieldMeta {
					name: "level_min".to_string(),
					rust_type: "Option<u8>".to_string(),
					is_required: false,
					is_sources: false,
					doc: "minimal zoom level".to_string(),
					enum_variants: Vec::new(),
				},
				VPLFieldMeta {
					name: "level_max".to_string(),
					rust_type: "Option<u8>".to_string(),
					is_required: false,
					is_sources: false,
					doc: "maximal zoom level".to_string(),
					enum_variants: Vec::new(),
				},
			],
		}];

		let ts = generate_typescript(&ops);
		assert!(ts.contains("export interface FilterOptions"));
		assert!(ts.contains("levelMin?: number;"));
		assert!(ts.contains("levelMax?: number;"));
		// All optional => options param is optional
		assert!(ts.contains("filter(options?: FilterOptions): VPL"));
	}

	#[test]
	fn test_generate_typescript_sources_op() {
		let ops = vec![OperationMeta {
			tag_name: "from_stacked_raster".to_string(),
			kind: "read",
			doc: "Stack raster sources.".to_string(),
			fields: vec![
				VPLFieldMeta {
					name: "sources".to_string(),
					rust_type: "Vec<VPLPipeline>".to_string(),
					is_required: true,
					is_sources: true,
					doc: "Raster sources.".to_string(),
					enum_variants: Vec::new(),
				},
				VPLFieldMeta {
					name: "format".to_string(),
					rust_type: "Option<TileFormat>".to_string(),
					is_required: false,
					is_sources: false,
					doc: "Output format.".to_string(),
					enum_variants: vec!["avif", "jpg", "png", "webp"],
				},
			],
		}];

		let ts = generate_typescript(&ops);
		assert!(ts.contains(
			"static fromStackedRaster(sources: VPL[], options?: Omit<FromStackedRasterOptions, 'sources'>): VPL"
		));
		// Enum-typed field is rendered as a TS string-literal union, not `string`.
		assert!(
			ts.contains(r#"format?: "avif" | "jpg" | "png" | "webp";"#),
			"expected literal union for format field, got:\n{ts}"
		);
	}

	#[test]
	fn live_metadata_emits_string_literal_union_for_existing_enum_op() {
		// from_stacked_raster::format is already typed `Option<TileFormat>`, so
		// after Chunk A this real op should produce a literal union — without
		// any per-op migration. Guards against the derive losing the
		// `enum_variants` wiring.
		let ops = versatiles::pipeline::all_operation_metadata();
		let op = ops
			.iter()
			.find(|o| o.tag_name == "from_stacked_raster")
			.expect("from_stacked_raster registered");
		let format_field = op.fields.iter().find(|f| f.name == "format").expect("format field");
		assert!(
			!format_field.enum_variants.is_empty(),
			"Option<TileFormat> field should carry enum_variants"
		);
		assert!(format_field.enum_variants.contains(&"png"));
	}

	/// Look up `(op_name, field_name)` in the live VPL metadata.
	fn find_field(ops: &[OperationMeta], op_name: &str, field_name: &str) -> VPLFieldMeta {
		let op = ops
			.iter()
			.find(|o| o.tag_name == op_name)
			.unwrap_or_else(|| panic!("operation `{op_name}` not registered"));
		op.fields
			.iter()
			.find(|f| f.name == field_name)
			.unwrap_or_else(|| panic!("field `{field_name}` not found on `{op_name}`"))
			.clone()
	}

	/// All VPL fields that should produce a TS string-literal union after
	/// Chunk B. One row per migrated arg; the canonical-variants list is
	/// pinned here so the test fails loudly if a new variant is added to an
	/// enum but the migration doc / TS surface forgets to follow.
	const ENUM_FIELDS: &[(&str, &str, &[&str])] = &[
		("from_color", "format", &["avif", "bin", "geojson", "jpg", "json", "mvt", "png", "svg", "topojson", "webp"]),
		("from_debug", "format", &["avif", "bin", "geojson", "jpg", "json", "mvt", "png", "svg", "topojson", "webp"]),
		("from_geo", "compression", &["none", "gzip", "brotli", "zstd"]),
		("from_geo", "point_reduction", &["none", "drop_rate", "min_distance"]),
		("from_csv", "compression", &["none", "gzip", "brotli", "zstd"]),
		("from_csv", "point_reduction", &["none", "drop_rate", "min_distance"]),
		("raster_format", "format", &["avif", "jpg", "png", "webp"]),
	];

	#[test]
	fn migrated_fields_carry_canonical_enum_variants_in_metadata() {
		// One pinned canonical-variants list per migrated field. Catches both
		// "field accidentally not migrated" (empty enum_variants) and "enum
		// variant added without updating the TS surface".
		let ops = versatiles::pipeline::all_operation_metadata();
		for (op_name, field_name, expected) in ENUM_FIELDS {
			let f = find_field(&ops, op_name, field_name);
			assert_eq!(
				f.enum_variants.as_slice(),
				*expected,
				"{op_name}::{field_name} variants drifted",
			);
		}
	}

	#[test]
	fn migrated_fields_render_as_string_literal_union_in_generated_typescript() {
		// End-to-end check: the .ts output for each migrated op carries a real
		// string-literal union, not `string`. Pulls the same TS that the napi
		// entry point (`generate_vpl_typescript`) returns.
		let ts = generate_vpl_typescript();
		for (op_name, field_name, variants) in ENUM_FIELDS {
			let camel = to_camel_case(field_name);
			let union = variants
				.iter()
				.map(|v| format!(r#""{v}""#))
				.collect::<Vec<_>>()
				.join(" | ");
			let needle = format!("\t{camel}?: {union};");
			assert!(
				ts.contains(&needle),
				"{op_name}::{field_name} should render as `{needle}` in the generated TS",
			);
		}
	}
}
