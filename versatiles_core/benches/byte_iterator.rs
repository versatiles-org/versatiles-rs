use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::io::Cursor;
use versatiles::utils::ByteIterator;

const DATA_SIZE: usize = 100 * 1024 * 1024;
const BATCH_SIZE: BatchSize = BatchSize::NumIterations(1);

fn bench_naked(c: &mut Criterion) {
	c.bench_function("ByteIterator data.iter.next", |b| {
		let data = vec![b'a'; DATA_SIZE];
		b.iter_batched(
			|| data.clone().into_iter(),
			|mut iter| {
				for _ in 0..DATA_SIZE {
					iter.next();
				}
			},
			BATCH_SIZE,
		)
	});
}

fn bench_advance(c: &mut Criterion) {
	c.bench_function("ByteIterator advance", |b| {
		let data = vec![b'a'; DATA_SIZE];
		b.iter_batched(
			|| ByteIterator::from_iterator(data.clone().into_iter(), false),
			|mut byte_iter| {
				for _ in 0..DATA_SIZE {
					byte_iter.advance();
				}
			},
			BATCH_SIZE,
		)
	});
}

fn bench_consume(c: &mut Criterion) {
	c.bench_function("ByteIterator consume", |b| {
		let data = vec![b'a'; DATA_SIZE];
		b.iter_batched(
			|| ByteIterator::from_iterator(data.clone().into_iter(), false),
			|mut byte_iter| {
				for _ in 0..DATA_SIZE {
					byte_iter.consume();
				}
			},
			BATCH_SIZE,
		)
	});
}

fn bench_skip_whitespace(c: &mut Criterion) {
	c.bench_function("ByteIterator skip_whitespace", |b| {
		let data = [vec![b' '; DATA_SIZE], vec![b'A'; 64]].concat();
		b.iter_batched(
			|| ByteIterator::from_iterator(data.clone().into_iter(), false),
			|mut byte_iter| byte_iter.skip_whitespace(),
			BATCH_SIZE,
		)
	});
}

fn bench_iter_into_string(c: &mut Criterion) {
	c.bench_function("ByteIterator iterator.into_string", |b| {
		let data = vec![b'a'; DATA_SIZE];
		b.iter_batched(
			|| ByteIterator::from_iterator(data.clone().into_iter(), false),
			|byte_iter| byte_iter.into_string(),
			BATCH_SIZE,
		)
	});
}

fn bench_iter_debug_into_string(c: &mut Criterion) {
	c.bench_function("ByteIterator iterator[debug].into_string", |b| {
		let data = vec![b'a'; DATA_SIZE];
		b.iter_batched(
			|| ByteIterator::from_iterator(data.clone().into_iter(), true),
			|byte_iter| byte_iter.into_string(),
			BATCH_SIZE,
		)
	});
}

fn bench_reader_into_string(c: &mut Criterion) {
	c.bench_function("ByteIterator reader.into_string", |b| {
		let data = vec![b'a'; DATA_SIZE];
		b.iter_batched(
			|| ByteIterator::from_reader(Cursor::new(data.clone()), false),
			|byte_iter| byte_iter.into_string(),
			BATCH_SIZE,
		)
	});
}

criterion_group!(
	name = benches;
	config = Criterion::default().significance_level(0.1).sample_size(10);
	targets = bench_naked, bench_advance, bench_consume, bench_skip_whitespace, bench_iter_into_string, bench_iter_debug_into_string, bench_reader_into_string
);
criterion_main!(benches);
