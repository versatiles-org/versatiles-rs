#!/usr/bin/env bash
cd "$(dirname "$0")"
cd ..

RUST_BACKTRACE=full cargo bench --bench versatiles
