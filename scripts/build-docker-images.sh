#!/usr/bin/env bash
cd "$(dirname "$0")/../docker"

DEBUG=false
ERROR=false

function docker_build_release()  {
	linux=$1
	platf=$2

	echo -e "\e[32;1m$linux on $platf\e[0m"

	echo -e "\e[32m  - build\e[0m"

	if [ -z ${linux+x} ]; then
		echo "linux is unset"
		exit 1
	fi
	if [ -z ${platf+x} ]; then
		echo "platf is unset"
		exit 1
	fi

	if $DEBUG; then
		docker buildx build --platform="${platf}" --progress="plain" --tag="${linux}-versatiles" --file="${linux}.Dockerfile" .
	else
		docker buildx build --platform="${platf}" --quiet --tag="${linux}-versatiles" --file="${linux}.Dockerfile" .
	fi

	echo -e "\e[32m  - test\e[0m"
	docker run --platform="${platf}" "${linux}-versatiles" sh selftest-versatiles.sh

	if [ "$?" != "0" ]; then
		echo -e "\e[31;1mERROR!\e[0m"
		ERROR=true
	fi
}

docker_build_release debian linux/amd64
docker_build_release alpine linux/amd64
docker_build_release scratch linux/amd64

if $ERROR; then
	echo -e "\e[31;1mTHERE WERE ERRORS!!!\e[0m"
	exit 1
else
	echo -e "\e[32;1moperating within normal parameters\e[0m"
fi
