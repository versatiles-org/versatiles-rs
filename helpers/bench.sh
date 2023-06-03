#!/usr/bin/env bash
cd "$(dirname "$0")"
cd ..

cargo bench --bench versatiles
