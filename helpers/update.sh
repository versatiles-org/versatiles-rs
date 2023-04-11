rustup update

# Find unused dependencies
cargo +nightly udeps

# check features
#unused-features analyze

# upgrade dependencies
cargo upgrades

# Update dependencies in the local lock file
cargo update
