use crate::test_utilities::{
	get_temp_output, get_testdata, tilejson, versatiles_output, versatiles_run, versatiles_stdin,
};

#[test]
fn e2e_reduce_print_writes_a_pipeline_to_stdout() {
	let style = get_testdata("styles/colorful.json");
	let input = get_testdata("berlin.mbtiles");
	let output = versatiles_output(&format!("reduce --print -s {style} {input}"));

	assert!(output.success, "stderr: {}", output.stderr);
	for expected in ["from_container filename=", "vector_reduce_to_style style="] {
		assert_contains!(output.stdout.clone(), expected);
	}
}

/// The workflow Studio depends on: the printed pipeline is VPL that `convert` accepts on stdin.
///
/// Piped rather than passed as an argument, which is both the documented idiom for inline VPL
/// (`versatiles help source`) and the only one that survives an expression containing spaces.
#[test]
fn e2e_a_printed_pipeline_can_be_piped_into_convert() {
	let style = get_testdata("styles/colorful.json");
	let input = get_testdata("berlin.mbtiles");
	let printed = versatiles_output(&format!("reduce --print -s {style} {input}"));
	assert!(printed.success, "stderr: {}", printed.stderr);

	let (_dir, out) = get_temp_output("piped.versatiles");
	versatiles_stdin(
		&format!("convert [,vpl]- {}", out.to_str().unwrap()),
		printed.stdout.trim(),
	);

	assert!(
		std::fs::metadata(&out).unwrap().len() > 0,
		"piping the printed pipeline into convert should produce a container"
	);
}

/// Reducing shrinks the tileset.
///
/// Both sides are written as `.versatiles` from the same source, because comparing against the
/// `.mbtiles` input would measure the container format rather than the reduction.
#[test]
fn e2e_reduce_writes_a_smaller_container() {
	let style = get_testdata("styles/colorful.json");
	let input = get_testdata("berlin.mbtiles");
	let (_dir, plain) = get_temp_output("plain.versatiles");
	let (_dir2, reduced) = get_temp_output("reduced.versatiles");

	versatiles_run(&format!("convert {input} {}", plain.to_str().unwrap()));
	versatiles_run(&format!("reduce -s {style} {input} {}", reduced.to_str().unwrap()));

	let size = std::fs::metadata(&reduced).unwrap().len();
	let original = std::fs::metadata(&plain).unwrap().len();
	assert!(size > 0, "the reduced container should not be empty");
	assert!(
		size < original,
		"reducing should shrink the tileset: {size} vs {original}"
	);
}

/// The headline case: `colorful` reads `name` and no `name_*` at all, so every translation goes.
///
/// `boundary_labels` in `berlin.mbtiles` carries `name`, `name_de` and `name_en`. Asserting both
/// halves matters — that the translations are gone *and* that `name` survived — because a reduction
/// that dropped all three would pass the first assertion while having destroyed the labels.
#[test]
fn e2e_reduce_drops_unused_translations_and_keeps_what_is_drawn() {
	let style = get_testdata("styles/colorful.json");
	let input = get_testdata("berlin.mbtiles");
	let (_dir, output) = get_temp_output("reduced.versatiles");

	versatiles_run(&format!("reduce -s {style} {input} {}", output.to_str().unwrap()));

	let json = tilejson(&output).stringify();
	assert!(!json.contains("name_de"), "the German translation is dropped");
	assert!(!json.contains("name_en"), "the English translation is dropped");
	assert!(json.contains("boundary_labels"), "the layer itself survives");
	assert!(json.contains(r#""name""#), "the name the style does read survives");
}

#[test]
fn e2e_reduce_without_an_output_file_fails_clearly() {
	let style = get_testdata("styles/colorful.json");
	let input = get_testdata("berlin.mbtiles");
	let output = versatiles_output(&format!("reduce -s {style} {input}"));

	assert!(!output.success);
	assert_contains!(output.stderr, "output file is required");
}

#[test]
fn e2e_reduce_reports_a_missing_style() {
	let input = get_testdata("berlin.mbtiles");
	let (_dir, out) = get_temp_output("reduced.versatiles");
	let output = versatiles_output(&format!(
		"reduce -s does-not-exist.json {input} {}",
		out.to_str().unwrap()
	));

	assert!(!output.success);
	assert_contains!(output.stderr, "reading 'style'");
}
