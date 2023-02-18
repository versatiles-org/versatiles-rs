#!/usr/bin/env bash
cd "$(dirname "$0")"

# check if version was updated
echo "Did you update the version number in Cargo.toml?"
select answer in "Yes" "No"; do
	if [ $answer != "Yes" ]; then
		echo "❗️ Then do it!"
		exit 1
	fi
done

# check if nothing to commit
if [ "$(git status --porcelain)" ]; then
	echo "❗️ Please commit all uncommitted changes!"
	exit 1
fi

# check cargo
./bin/check.sh
if [ $? -ne 0 ]; then
	echo "❗️ Check failed!"
	exit 1
fi

# publish
cargo publish --no-verify

# get version
version=$(cat Cargo.toml | sed -ne 's/^version[ ="]*\([0-9\.]*\).*$/\1/p')

# git tag
git tag -a "v$version" -m "new release: v$version"


