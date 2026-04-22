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

/// Map a Rust type string to a TypeScript type string.
fn rust_type_to_ts(rust_type: &str) -> &'static str {
	match rust_type {
		"String" | "Option<String>" | "Option<TileFormat>" | "Option<TileSchema>" | "Option<TileCompression>" => "string",
		"bool" | "Option<bool>" => "boolean",
		"u8" | "u16" | "u32" | "f32" | "f64" | "Option<u8>" | "Option<u16>" | "Option<u32>" | "Option<f32>"
		| "Option<f64>" => "number",
		"[f64;4]" | "Option<[f64;4]>" => "[number, number, number, number]",
		"[f64;3]" | "Option<[f64;3]>" | "[u8;3]" | "Option<[u8;3]>" => "[number, number, number]",
		"Vec<VPLPipeline>" => "VPL[]",
		_ => "unknown",
	}
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
		let ts_type = rust_type_to_ts(&field.rust_type);
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
			writeln!(out, "\tstatic {method_name}(options?: {interface_name}): VPL {{").expect("writing to string never fails");
		}
		(false, true) => {
			writeln!(out, "\tstatic {method_name}(options: {interface_name}): VPL {{").expect("writing to string never fails");
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
		writeln!(out, "\t\treturn new VPL([{{ name: '{tag}', params, sources }}]);").expect("writing to string never fails");
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

	writeln!(out, "\t\treturn new VPL([...this.steps, {{ name: '{tag}', params }}]);").expect("writing to string never fails");

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

	#[test]
	fn test_rust_type_to_ts() {
		assert_eq!(rust_type_to_ts("String"), "string");
		assert_eq!(rust_type_to_ts("bool"), "boolean");
		assert_eq!(rust_type_to_ts("u8"), "number");
		assert_eq!(rust_type_to_ts("Option<u8>"), "number");
		assert_eq!(rust_type_to_ts("Option<String>"), "string");
		assert_eq!(rust_type_to_ts("[f64;4]"), "[number, number, number, number]");
		assert_eq!(rust_type_to_ts("Option<[f64;3]>"), "[number, number, number]");
		assert_eq!(rust_type_to_ts("Option<TileFormat>"), "string");
		assert_eq!(rust_type_to_ts("Vec<VPLPipeline>"), "VPL[]");
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
				},
				VPLFieldMeta {
					name: "level_max".to_string(),
					rust_type: "Option<u8>".to_string(),
					is_required: false,
					is_sources: false,
					doc: "maximal zoom level".to_string(),
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
				},
				VPLFieldMeta {
					name: "format".to_string(),
					rust_type: "Option<TileFormat>".to_string(),
					is_required: false,
					is_sources: false,
					doc: "Output format.".to_string(),
				},
			],
		}];

		let ts = generate_typescript(&ops);
		assert!(ts.contains(
			"static fromStackedRaster(sources: VPL[], options?: Omit<FromStackedRasterOptions, 'sources'>): VPL"
		));
	}
}
