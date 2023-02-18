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

git push

# publish
cargo publish --no-verify

# get version
version=$(cat Cargo.toml | sed -ne 's/^version[ ="]*\([0-9\.]*\).*$/\1/p')

# git tag
git tag -a "v$version" -m "new release: v$version"
git push origin "v$version"

# github create release
gh release create "v$version" --title "v$version" --notes "new release: v$version"


# https://doc.rust-lang.org/nightly/rustc/platform-support.html


mkdir "releases"

function release() {
	target=$1
	cargo build --release --target $target
	mv "target/$target/release/versatiles" "releases/versatiles-$target"
}

release "aarch64-unknown-linux-gnu" # ARM64 Linux (kernel 4.1, glibc 2.17+)
release "i686-pc-windows-gnu"       # 32-bit MinGW (Windows 7+)
release "i686-pc-windows-msvc"      # 32-bit MSVC (Windows 7+)
release "i686-unknown-linux-gnu"    # 32-bit Linux (kernel 3.2+, glibc 2.17+)
release "x86_64-apple-darwin"       # 64-bit macOS (10.7+, Lion+)
release "x86_64-pc-windows-gnu"     # 64-bit MinGW (Windows 7+)
release "x86_64-pc-windows-msvc"    # 64-bit MSVC (Windows 7+)
release "x86_64-unknown-linux-gnu"  # 64-bit Linux (kernel 3.2+, glibc 2.17+)
