use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::{hint::black_box, io::Cursor};
use versatiles_core::utils::read_csv_iter;

const DATA_SIZE: usize = 16 * 1024 * 1024;

fn large_csv_data() -> String {
	let mut csv_data = String::from("name,age\n");
	let mut i: u64 = 0;
	while csv_data.len() < DATA_SIZE {
		i += 1;
		csv_data.push_str(&format!("\"John, {} Doe\",{}\n", i, 20 + i % 50));
	}
	csv_data
}

fn benchmark_read_csv_iter(c: &mut Criterion) {
	let data = large_csv_data();
	let cursor = Cursor::new(data);

	c.bench_function("read_csv_iter", |b| {
		b.iter_batched(
			|| read_csv_iter(cursor.clone(), b',').unwrap(),
			|iter| {
				for row in black_box(iter) {
					row.unwrap();
				}
			},
			BatchSize::NumIterations(1),
		);
	});
}

criterion_group!(
	name = benches;
	config = Criterion::default().significance_level(0.1).sample_size(15);
	targets = benchmark_read_csv_iter
);
criterion_main!(benches);
