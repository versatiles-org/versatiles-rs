#!/usr/bin/env bash
cd "$(dirname "$0")/.."
PROJECT_DIR=$(pwd)

packages=(
	"versatiles"
	"versatiles_container"
	"versatiles_core"
	"versatiles_derive"
	"versatiles_geometry"
	"versatiles_image"
	"versatiles_pipeline"
)

for package in "${packages[@]}"; do
	cd $PROJECT_DIR/$package
	echo "$package"
	cargo +nightly rustdoc -- -Z unstable-options --show-coverage 2> /dev/null
done
