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

# docker_build_release debian linux/amd64
docker_build_release debian linux/arm64/v8

#docker build --progress=plain -t debian-versatiles -f debian.Dockerfile .
#docker build --progress=plain -t debian-versatiles-test -f debian.test.Dockerfile .
#docker run --rm debian-versatiles-test