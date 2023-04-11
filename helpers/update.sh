echo "Update rust"
rustup update

echo "Find unused dependencies"
cargo +nightly udeps

#echo "check features"
#unused-features analyze

echo "upgrade dependencies"
cargo upgrades

echo "Update dependencies in the local lock file"
cargo update
