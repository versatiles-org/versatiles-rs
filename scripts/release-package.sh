#!/usr/bin/env bash
cd "$(dirname "$0")/.."
set -e

# Color codes
RED="\033[1;31m"
GRE="\033[1;32m"
YEL="\033[1;33m"
BLU="\033[1;34m"
END="\033[0m"

# Valid release type keywords
VALID_KEYWORDS="patch minor major alpha beta rc"

# Helper function for logging steps
log_step() {
	echo -e "${BLU}▸ $1${END}"
}

log_success() {
	echo -e "${GRE}✓ $1${END}"
}

log_error() {
	echo -e "${RED}❗️ $1${END}"
}

# Get current version from Cargo.toml
get_current_version() {
	grep -m1 '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/'
}

# Calculate new version using npm for keywords
calculate_new_version() {
	local release_type="$1"
	local new_version

	cd versatiles_node
	case "$release_type" in
		patch|minor|major)
			new_version=$(npm version "$release_type" --no-git-tag-version 2>&1 | grep '^v' | sed 's/^v//')
			;;
		rc|alpha|beta)
			new_version=$(npm version prerelease --preid="$release_type" --no-git-tag-version 2>&1 | grep '^v' | sed 's/^v//')
			;;
		*)
			log_error "Invalid release type: $release_type"
			exit 1
			;;
	esac

	# Revert npm's changes
	git checkout package.json package-lock.json 2>/dev/null
	cd ..

	echo "$new_version"
}

# Validate specific version string
validate_specific_version() {
	local version="$1"
	local current_version="$2"

	# Validate it's a valid semver
	cd versatiles_node
	if ! npm version "$version" --no-git-tag-version --allow-same-version 2>/dev/null; then
		git checkout package.json package-lock.json 2>/dev/null
		cd ..
		log_error "Invalid semver version: $version"
		exit 1
	fi
	git checkout package.json package-lock.json 2>/dev/null
	cd ..

	# Check it's not the same as current
	if [ "$version" = "$current_version" ]; then
		log_error "New version must be different from current version ($current_version)"
		exit 1
	fi

	# Check it's greater than current version using Node.js
	# Try to use semver package, fallback to basic comparison
	local is_greater
	if command -v node >/dev/null 2>&1; then
		# Try with semver package
		is_greater=$(node -e "
			try {
				const semver = require('semver');
				console.log(semver.gt('$version', '$current_version'));
			} catch (e) {
				// Fallback: basic string comparison (not perfect but better than nothing)
				const compareVersions = (v1, v2) => {
					const parts1 = v1.split(/[-.]/).map(p => parseInt(p) || p);
					const parts2 = v2.split(/[-.]/).map(p => parseInt(p) || p);
					for (let i = 0; i < Math.max(parts1.length, parts2.length); i++) {
						const p1 = parts1[i] || 0;
						const p2 = parts2[i] || 0;
						if (p1 > p2) return true;
						if (p1 < p2) return false;
					}
					return false;
				};
				console.log(compareVersions('$version', '$current_version'));
			}
		" 2>/dev/null)

		if [ "$is_greater" != "true" ]; then
			log_error "New version ($version) must be greater than current version ($current_version)"
			exit 1
		fi
	else
		log_error "Node.js not found, cannot validate version ordering"
		exit 1
	fi

	log_success "Version validation passed: $version > $current_version"
}

# Update Cargo.toml version in two locations
update_cargo_toml_version() {
	local new_version="$1"

	log_step "Updating Cargo.toml..."

	# Update workspace.package.version (around line 28)
	sed -i.bak "s/^version = \".*\"/version = \"$new_version\"/" Cargo.toml

	# Update all workspace dependencies (around lines 156-162)
	# Pattern: versatiles* = { version = "X.Y.Z", path = ...
	sed -i.bak "s/\(versatiles[^=]*= { version = \)\"[^\"]*\"/\1\"$new_version\"/" Cargo.toml

	# Remove backup files
	rm -f Cargo.toml.bak

	log_success "Cargo.toml updated to version $new_version"
}

# Update package.json version
update_package_json_version() {
	local new_version="$1"

	log_step "Updating package.json..."

	cd versatiles_node
	npm version "$new_version" --no-git-tag-version --allow-same-version >/dev/null 2>&1
	cd ..

	log_success "package.json updated to version $new_version"
}

# Update Cargo.lock
update_cargo_lock() {
	log_step "Updating Cargo.lock..."
	cargo check --quiet 2>/dev/null || cargo check 2>&1 | grep -v "Compiling\|Finished" || true
	log_success "Cargo.lock updated"
}

# Create release commit
create_release_commit() {
	local new_version="$1"

	log_step "Creating release commit..."

	git add Cargo.toml Cargo.lock versatiles_node/package.json versatiles_node/package-lock.json

	git commit -S -m "release: v$new_version"

	log_success "Commit created: release: v$new_version"
}

# Create release tag
create_release_tag() {
	local new_version="$1"

	log_step "Creating release tag..."

	git tag -s "v$new_version" -m "Release v$new_version"

	log_success "Tag created: v$new_version"
}

# Main function
main() {
	local version_arg="$1"
	local release_type=""
	local new_version=""
	local is_specific_version=false

	# Semver regex pattern
	local version_regex='^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.-]+)?$'

	# Parse and validate argument
	if [ -z "$version_arg" ]; then
		# Interactive menu
		echo -e "${BLU}Select release type:${END}"
		echo ""

		PS3=$'\nEnter selection number: '
		options=(
			"patch   - Bug fixes, small improvements (x.y.Z)"
			"minor   - New features, backward compatible (x.Y.0)"
			"major   - Breaking changes (X.0.0)"
			"alpha   - Early development, unstable API (x.y.z-alpha.N)"
			"beta    - Feature complete, testing phase (x.y.z-beta.N)"
			"rc      - Release candidate, final testing (x.y.z-rc.N)"
			"Cancel"
		)

		select opt in "${options[@]}"; do
			case $REPLY in
				1) release_type="patch"; break;;
				2) release_type="minor"; break;;
				3) release_type="major"; break;;
				4) release_type="alpha"; break;;
				5) release_type="beta"; break;;
				6) release_type="rc"; break;;
				7) echo -e "${YEL}Cancelled${END}"; exit 0;;
				*) echo -e "${RED}Invalid selection${END}";;
			esac
		done

		echo ""
		echo -e "${GRE}Selected: $release_type${END}"
		echo ""
	elif [[ "$version_arg" =~ $version_regex ]]; then
		# Specific version provided
		is_specific_version=true
		new_version="$version_arg"
		echo -e "${BLU}Using specific version: $new_version${END}"
		echo ""
	else
		# Release type keyword
		if ! echo "$VALID_KEYWORDS" | grep -wq "$version_arg"; then
			log_error "Invalid argument: $version_arg"
			echo "Must be one of: $VALID_KEYWORDS"
			echo "Or a specific version like: 3.0.0-rc.3"
			exit 1
		fi
		release_type="$version_arg"
		echo -e "${BLU}Using release type: $release_type${END}"
		echo ""
	fi

	# Pre-release checks
	log_step "Building README documentation..."
	./scripts/build-docs-readme.sh

	log_step "Checking git status..."
	if [ -n "$(git status --porcelain)" ]; then
		log_error "Git working directory is not clean!"
		git status --porcelain
		exit 1
	fi
	log_success "Git working directory is clean"

	log_step "Running checks..."
	./scripts/check.sh
	if [ $? -ne 0 ]; then
		log_error "Checks failed!"
		exit 1
	fi
	log_success "Checks passed"

	# Get current version
	local current_version
	current_version=$(get_current_version)
	echo ""
	echo -e "${BLU}Current version: $current_version${END}"

	# Calculate or validate new version
	if [ "$is_specific_version" = true ]; then
		validate_specific_version "$new_version" "$current_version"
	else
		log_step "Calculating new version..."
		new_version=$(calculate_new_version "$release_type")
		log_success "Calculated new version: $new_version"
	fi

	echo ""
	echo -e "${BLU}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
	echo -e "${BLU}Releasing version: $new_version${END}"
	echo -e "${BLU}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
	echo ""

	# Update version files
	update_cargo_toml_version "$new_version"
	update_package_json_version "$new_version"
	update_cargo_lock

	echo ""

	# Create commit and tag
	create_release_commit "$new_version"
	create_release_tag "$new_version"

	echo ""
	log_step "Pushing to remote..."
	git push origin main --follow-tags

	echo ""
	echo -e "${GRE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
	echo -e "${GRE}✓ Release v$new_version completed successfully!${END}"
	echo -e "${GRE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
}

main "$@"
