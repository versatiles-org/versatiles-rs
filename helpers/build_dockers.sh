#!/usr/bin/env bash
cd "$(dirname "$0")"
cd ../docker

set -e

function docker_build_release () {
	linux=$1
	platf=$2
	if [ -z ${linux+x} ]; then echo "linux is unset"; exit 1; fi
	if [ -z ${platf+x} ]; then echo "platf is unset"; exit 1; fi

	docker buildx build --platform="${platf}" --progress="plain" --tag="${linux}-versatiles" --file="${linux}.Dockerfile" .
}

docker_build_release debian linux/amd64
docker_build_release alpine linux/amd64
docker_build_release scratch linux/amd64
