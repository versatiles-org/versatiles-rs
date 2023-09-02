#!/usr/bin/env bash

cargo +nightly udeps --quiet --bin versatiles
cargo +nightly udeps --quiet --bin versatiles --no-default-features
cargo +nightly udeps --quiet --bin versatiles --no-default-features --features mbtiles
cargo +nightly udeps --quiet --bin versatiles --no-default-features --features mbtiles server
cargo +nightly udeps --quiet --bin versatiles --no-default-features --features mbtiles server tar
cargo +nightly udeps --quiet --bin versatiles --no-default-features --features mbtiles tar
cargo +nightly udeps --quiet --bin versatiles --no-default-features --features server
cargo +nightly udeps --quiet --bin versatiles --no-default-features --features server tar
cargo +nightly udeps --quiet --bin versatiles --no-default-features --features tar
cargo +nightly udeps --quiet --lib
cargo +nightly udeps --quiet --lib --no-default-features
