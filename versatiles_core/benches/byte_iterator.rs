use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use std::io::Cursor;
use versatiles_core::utils::ByteIterator;

const DATA_SIZE: usize = 100 * 1024 * 1024;
const BATCH_SIZE: BatchSize = BatchSize::NumIterations(1);

fn bench_advance(c: &mut Criterion) {
	c.bench_function("ByteIterator advance", |b| {
		let reader = Cursor::new(vec![b'a'; DATA_SIZE]);
		b.iter_batched(
			|| ByteIterator::from_reader(reader.clone(), false),
			|mut byte_iter| {
				for _ in 0..DATA_SIZE {
					byte_iter.advance();
					black_box(());
				}
			},
			BATCH_SIZE,
		)
	});
}

fn bench_consume(c: &mut Criterion) {
	c.bench_function("ByteIterator consume", |b| {
		let reader = Cursor::new(vec![b'a'; DATA_SIZE]);
		b.iter_batched(
			|| ByteIterator::from_reader(reader.clone(), false),
			|mut byte_iter| {
				for _ in 0..DATA_SIZE {
					black_box(byte_iter.consume());
				}
			},
			BATCH_SIZE,
		)
	});
}

fn bench_skip_whitespace(c: &mut Criterion) {
	c.bench_function("ByteIterator skip_whitespace", |b| {
		let data = [vec![b' '; DATA_SIZE], vec![b'A'; 64]].concat();
		let reader = Cursor::new(data);
		b.iter_batched(
			|| ByteIterator::from_reader(reader.clone(), false),
			|mut byte_iter| {
				byte_iter.skip_whitespace();
				black_box(())
			},
			BATCH_SIZE,
		)
	});
}

fn bench_into_string(c: &mut Criterion) {
	c.bench_function("ByteIterator into_string", |b| {
		let reader = Cursor::new(vec![b'a'; DATA_SIZE]);
		b.iter_batched(
			|| ByteIterator::from_reader(reader.clone(), false),
			|byte_iter| black_box(byte_iter.into_string()),
			BATCH_SIZE,
		)
	});

	c.bench_function("ByteIterator [debug].into_string", |b| {
		let reader = Cursor::new(vec![b'a'; DATA_SIZE]);
		b.iter_batched(
			|| ByteIterator::from_reader(reader.clone(), true),
			|byte_iter| black_box(byte_iter.into_string()),
			BATCH_SIZE,
		)
	});
}

criterion_group!(
	name = benches;
	config = Criterion::default().significance_level(0.1).sample_size(10);
	targets =  bench_advance, bench_consume, bench_skip_whitespace, bench_into_string
);
criterion_main!(benches);
