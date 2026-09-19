use crate::test_utilities::{get_temp_output, get_testdata, versatiles_output, versatiles_run, versatiles_stdin};

#[test]
fn e2e_reduce_print_writes_a_pipeline_to_stdout() {
	let requirement = get_testdata("reduce/streets.json");
	let input = get_testdata("berlin.versatiles");
	let output = versatiles_output(&format!("reduce --print -r {requirement} {input}"));

	assert!(output.success, "stderr: {}", output.stderr);
	for expected in [
		"from_container filename=",
		"vector_filter_layers",
		"vector_filter_properties",
	] {
		assert_contains!(output.stdout.clone(), expected);
	}
}

/// The workflow Studio depends on: the printed pipeline is VPL that `convert` accepts on stdin.
///
/// Piped rather than passed as an argument, which is both the documented idiom for inline VPL
/// (`versatiles help source`) and the only one that survives an expression containing spaces.
#[test]
fn e2e_a_printed_pipeline_can_be_piped_into_convert() {
	let requirement = get_testdata("reduce/streets.json");
	let input = get_testdata("berlin.versatiles");
	let printed = versatiles_output(&format!("reduce --print -r {requirement} {input}"));
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

#[test]
fn e2e_reduce_writes_a_reduced_container() {
	let requirement = get_testdata("reduce/streets.json");
	let input = get_testdata("berlin.versatiles");
	let (_dir, output) = get_temp_output("reduced.versatiles");

	versatiles_run(&format!("reduce -r {requirement} {input} {}", output.to_str().unwrap()));

	let size = std::fs::metadata(&output).unwrap().len();
	assert!(size > 0, "the reduced container should not be empty");
	let original = std::fs::metadata(get_testdata("berlin.versatiles")).unwrap().len();
	assert!(
		size < original,
		"reducing should shrink the tileset: {size} vs {original}"
	);
}

#[test]
fn e2e_reduce_without_an_output_file_fails_clearly() {
	let requirement = get_testdata("reduce/minimal.json");
	let input = get_testdata("berlin.versatiles");
	let output = versatiles_output(&format!("reduce -r {requirement} {input}"));

	assert!(!output.success);
	assert_contains!(output.stderr, "output file is required");
}
