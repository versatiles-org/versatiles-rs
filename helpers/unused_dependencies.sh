#!/usr/bin/env bash

function run() {
	bin=$1
	features=$2
	cmd="cargo +nightly udeps --quiet"
	if [[ $bin =~ "bin" ]]; then
		cmd="$cmd --bin versatiles"
	else
		cmd="$cmd --lib"
	fi
	cmd="$cmd --no-default-features"
	if [[ -n "$features" ]]; then
		cmd="$cmd --features $features"
	fi
	cmd="$cmd 2>&1 | grep -vE 'info:' | sed 's/^/   /'"

	echo -e "\033[1;30mrun: $bin $features\033[0m"
	bash -c "$cmd"
}

run bin default

run bin cli
run bin cli,image
run bin cli,mbtiles
run bin cli,request
run bin cli,server
run bin cli,tar

run bin cli,image,mbtiles,request,server,tar

run bin cli,mbtiles,request,server,tar
run bin cli,image,request,server,tar
run bin cli,image,mbtiles,server,tar
run bin cli,image,mbtiles,request,tar
run bin cli,image,mbtiles,request,server

run lib
