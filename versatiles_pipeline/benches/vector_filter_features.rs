//! Where the time goes in `vector_filter_features`.
//!
//! The operation evaluates a CEL expression once per feature, and `FOLLOWUP_PLAN.md` lists three
//! guesses about what that costs. This exists so those stop being guesses.
//!
//! ## Reading the numbers
//!
//! Every case runs the same pipeline over the same tiles and differs only in `expr`, so the
//! absolute figures include reading and decoding tiles and are not interesting on their own. The
//! **differences between cases** are the measurement:
//!
//! - `zoom_only` minus `constant_true` is the cost of the per-feature path itself. Both expressions
//!   keep every feature and read nothing about it, but `zoom >= 0` takes the whole-tile shortcut
//!   added in #274 while `true` does not — so the gap is `Context::default()` plus one `execute`,
//!   multiplied by the feature count. That gap is what P-1 proposes to remove.
//! - `bare_identifier` minus `constant_true` is the cost of binding one variable.
//!
//! Every case is phrased to keep **every** feature. That is not incidental: a filter that drops
//! features re-encodes fewer of them, and since re-encoding costs more than evaluating, a
//! selective expression measures as *faster* and the saving is misread as its predicate being
//! cheap.
//! - `props_map` minus `bare_identifier` is the cost of rebuilding the `props` map per feature,
//!   which is what P-2 is about.
//! - `reduction_shape` is what a generated reduction pipeline actually runs, and the number to
//!   quote when saying whether any of this matters.
//!
//! The run also prints how many distinct property sets the tiles hold per feature, which is the
//! only thing that decides whether P-3 (memoising on identical properties) is worth its cost.

use std::{collections::HashSet, hint::black_box, path::PathBuf};

use criterion::{Criterion, criterion_group, criterion_main};
use versatiles_container::{TileSource, TilesRuntime};
use versatiles_core::TileBBox;
use versatiles_pipeline::PipelineReader;

/// Berlin at z13, cropped to a few tiles — enough features for the per-feature path to dominate,
/// few enough that a criterion sample does not take minutes.
const SOURCE: &str = r#"from_container filename="testdata/berlin.versatiles" | filter bbox=[13.35,52.49,13.45,52.53] level_min=13 level_max=13"#;

/// The zoom the pipeline is pinned to, so the bbox resolves to a handful of tiles.
const LEVEL: u8 = 13;

/// `streets` is the layer a reduction spends its time in: the most features, and the `kind`
/// property every style filters on.
const LAYER: &str = "streets";

fn runtime() -> tokio::runtime::Runtime {
	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.unwrap()
}

/// The workspace root, so `testdata/...` in the VPL resolves wherever the bench is run from.
fn workspace_root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Builds the pipeline against the real filesystem.
///
/// Not `PipelineFactory::new_dummy()`: its reader callback is a stub that cannot open a container,
/// so every case would measure the same error path.
async fn open(vpl: &str) -> PipelineReader {
	PipelineReader::open_str(vpl, &workspace_root(), TilesRuntime::new_silent())
		.await
		.unwrap_or_else(|e| panic!("building {vpl}: {e:?}"))
}

/// Streams every tile of an already-built pipeline, returning how many arrived.
///
/// The pipeline is built once per case and outside the timing loop: opening the container costs
/// far more than filtering it, and it is identical across cases, so leaving it in would add the
/// same large constant to every number and bury the differences that are the point.
async fn drain(reader: &PipelineReader) -> usize {
	reader
		.tile_stream(TileBBox::new_full(LEVEL).unwrap())
		.await
		.unwrap()
		.to_vec()
		.await
		.len()
}

/// The pipeline for one `expr`, or the bare source when `expr` is `None`.
fn pipeline(expr: Option<&str>) -> String {
	match expr {
		None => SOURCE.to_string(),
		Some(expr) => format!(r#"{SOURCE} | vector_filter_features layer=["{LAYER}"] expr="{expr}""#),
	}
}

/// Prints how much repetition the tiles hold, which is the whole case for P-3.
///
/// `PropertyManager` already deduplicates key/value *pairs*; this asks the different question of
/// how many distinct property *sets* there are. If it is close to the feature count, memoising on
/// them buys nothing and P-3 should be closed.
fn report_property_repetition(rt: &tokio::runtime::Runtime) {
	let (features, distinct) = rt.block_on(async {
		let reader = open(SOURCE).await;
		let tiles = reader
			.tile_stream(TileBBox::new_full(LEVEL).unwrap())
			.await
			.unwrap()
			.to_vec()
			.await;

		let mut features = 0usize;
		let mut distinct: HashSet<String> = HashSet::new();
		for (_coord, tile) in tiles {
			let Ok(vector) = tile.into_vector() else { continue };
			for layer in vector.layers.iter().filter(|l| l.name == LAYER) {
				for feature in layer.to_features().unwrap_or_default() {
					features += 1;
					// Stringified because `GeoProperties` is not `Hash`; this is a diagnostic, and
					// over-counting distinct sets would only understate the case for P-3.
					distinct.insert(format!("{:?}", feature.properties));
				}
			}
		}
		(features, distinct.len())
	});

	if features == 0 {
		eprintln!("property repetition: no features in `{LAYER}` — check the bbox in SOURCE");
		return;
	}
	eprintln!(
		"property repetition in `{LAYER}`: {distinct} distinct property sets across {features} features \
		 ({:.1}% repeated) — P-3 is worth considering only if this is high",
		100.0 - (distinct as f64 / features as f64) * 100.0
	);
}

fn benchmark_evaluate(c: &mut Criterion) {
	let rt = runtime();
	report_property_repetition(&rt);

	let cases: [(&str, Option<&str>); 6] = [
		// No filter at all: the floor every other case sits on.
		("source_only", None),
		// Keeps everything via the whole-tile shortcut: one evaluation per tile.
		("zoom_only", Some("zoom >= 0")),
		// Keeps everything the slow way: one evaluation per feature, empty context.
		("constant_true", Some("true")),
		// One bound variable. Phrased so it keeps every feature: an expression that filters some
		// out would re-encode fewer of them, and that saving would be scored as the variable
		// binding being cheap.
		("bare_identifier", Some("kind != 'no-such-kind'")),
		// The same question asked through the props map, which is rebuilt per feature.
		("props_map", Some("props.kind != 'no-such-kind'")),
		// What a generated reduction pipeline runs.
		(
			"reduction_shape",
			Some("(has(props.kind) && props.kind in ['motorway', 'trunk']) || zoom >= 12"),
		),
	];

	let mut group = c.benchmark_group("vector_filter_features");
	for (name, expr) in cases {
		let vpl = pipeline(expr);
		let reader = rt.block_on(open(&vpl));
		// Fail loudly here rather than measuring a pipeline that does nothing.
		assert!(rt.block_on(drain(&reader)) > 0, "{name} produced no tiles: {vpl}");

		group.bench_function(name, |b| {
			b.iter(|| black_box(rt.block_on(drain(&reader))));
		});
	}
	group.finish();
}

criterion_group!(
	name = benches;
	config = Criterion::default().significance_level(0.1).sample_size(15);
	targets = benchmark_evaluate
);
criterion_main!(benches);
