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
VALID_KEYWORDS="patch minor major release alpha beta rc"

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

# Calculate new version using cargo-release (dry-run mode)
calculate_new_version() {
	local release_type="$1"
	local new_version

	# cargo-release outputs: "Upgrading workspace to version X.Y.Z"
	new_version=$(cargo release version "$release_type" --workspace 2>&1 | \
		grep "Upgrading workspace to version" | \
		sed 's/.*version //')

	if [ -z "$new_version" ]; then
		log_error "Failed to calculate new version for release type: $release_type"
		exit 1
	fi

	echo "$new_version"
}

# Validate specific version string
validate_specific_version() {
	local version="$1"
	local current_version="$2"

	# Validate it's a valid semver using cargo-release
	if ! cargo release version "$version" --workspace 2>/dev/null | grep -q "Upgrading workspace to version"; then
		log_error "Invalid semver version: $version"
		exit 1
	fi

	# Check it's not the same as current
	if [ "$version" = "$current_version" ]; then
		log_error "New version must be different from current version ($current_version)"
		exit 1
	fi

	# Check it's greater than current version using Node.js + semver
	local is_greater
	if command -v node >/dev/null 2>&1; then
		is_greater=$(node -e "
			try {
				const semver = require('semver');
				console.log(semver.gt('$version', '$current_version'));
			} catch (e) {
				// Fallback: basic comparison
				const v1 = '$version'.split(/[-.]/).map(p => parseInt(p) || p);
				const v2 = '$current_version'.split(/[-.]/).map(p => parseInt(p) || p);
				for (let i = 0; i < Math.max(v1.length, v2.length); i++) {
					if ((v1[i] || 0) > (v2[i] || 0)) { console.log('true'); process.exit(0); }
					if ((v1[i] || 0) < (v2[i] || 0)) { console.log('false'); process.exit(0); }
				}
				console.log('false');
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

# Update Cargo.toml files and Cargo.lock using cargo-release
update_cargo_versions() {
	local new_version="$1"

	log_step "Updating Cargo.toml files and Cargo.lock..."

	# Use cargo-release version step to update all Cargo files
	# This updates:
	# - workspace.package.version in root Cargo.toml
	# - all workspace dependencies in root Cargo.toml
	# - version.workspace references in all crate Cargo.toml files
	# - Cargo.lock automatically
	# --execute: actually perform the changes (not dry-run)
	# --no-confirm: skip interactive confirmation
	cargo release version "$new_version" --execute --workspace --no-confirm 2>&1 | grep -v "Upgrading workspace to version" || true

	log_success "Cargo.toml files and Cargo.lock updated to version $new_version"
}

# Update package.json and package-lock.json version
update_package_json_version() {
	local new_version="$1"

	log_step "Updating package.json and package-lock.json..."

	# Use sync-version.sh to update package.json
	./scripts/sync-version.sh --fix

	# Verify both files were updated
	local pkg_version
	pkg_version=$(node -p "require('./versatiles_node/package.json').version")
	local lock_version
	lock_version=$(node -p "require('./versatiles_node/package-lock.json').version")

	if [ "$pkg_version" = "$new_version" ] && [ "$lock_version" = "$new_version" ]; then
		log_success "package.json and package-lock.json updated to version $new_version"
	else
		log_error "Failed to update package files correctly"
		echo "  package.json: $pkg_version"
		echo "  package-lock.json: $lock_version"
		echo "  expected: $new_version"
		exit 1
	fi
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

# Ensure prerequisites for the release workflow are present.
preflight_checks() {
	local current_branch
	current_branch=$(git rev-parse --abbrev-ref HEAD)
	if [ "$current_branch" != "dev" ]; then
		log_error "Releases must be cut from the 'dev' branch (currently on '$current_branch')"
		exit 1
	fi

	if [ -n "$(git status --porcelain)" ]; then
		log_error "Git working directory is not clean!"
		git status --porcelain
		exit 1
	fi

	if ! command -v gh >/dev/null 2>&1; then
		log_error "Required: GitHub CLI ('gh') for the post-push CI wait step"
		exit 1
	fi

	if ! gh auth status >/dev/null 2>&1; then
		log_error "Not logged in to GitHub CLI; run: gh auth login"
		exit 1
	fi

	log_step "Fetching latest refs from origin..."
	git fetch origin --quiet

	log_step "Ensuring 'dev' is at or ahead of 'origin/dev'..."
	if ! git merge-base --is-ancestor origin/dev HEAD; then
		log_error "Local 'dev' has diverged from or is behind 'origin/dev'. Pull/rebase first."
		exit 1
	fi

	log_step "Ensuring 'main' is an ancestor of 'dev' (no main-only commits)..."
	if ! git merge-base --is-ancestor origin/main HEAD; then
		log_error "'origin/main' is not an ancestor of 'dev' — sync first"
		log_error "Hint: git checkout dev && git merge --ff-only origin/main"
		exit 1
	fi
}

# Run build-docs-readme.sh and, if it modified any tracked files, land those
# changes as a standalone commit before the release commit.
regenerate_readmes() {
	log_step "Building README documentation..."
	./scripts/build-docs-readme.sh

	if [ -n "$(git status --porcelain)" ]; then
		log_step "Generated READMEs changed — committing as a separate commit..."
		git add -u
		git commit -S -m "docs: regenerate auto-built READMEs"
		log_success "Docs commit created"
	else
		log_success "Generated READMEs already up to date"
	fi
}

# Push 'dev', then watch the CI run on the just-pushed commit and exit non-zero
# if it fails. The 'main' branch protection requires a green 'CI Success' check
# on the tip — we can't fast-forward main until this passes.
push_dev_and_wait_for_ci() {
	local sha
	sha=$(git rev-parse HEAD)
	local short_sha
	short_sha=$(git rev-parse --short HEAD)

	log_step "Pushing 'dev' to origin..."
	git push origin dev

	log_step "Locating CI run for $short_sha..."
	local run_id=""
	for _ in 1 2 3 4 5 6; do
		run_id=$(gh run list \
			--repo versatiles-org/versatiles-rs \
			--branch dev \
			--commit "$sha" \
			--workflow CI \
			--limit 1 \
			--json databaseId \
			--jq '.[0].databaseId' 2>/dev/null || true)
		[ -n "$run_id" ] && break
		sleep 5
	done
	if [ -z "$run_id" ]; then
		log_error "Could not find CI run for commit $short_sha"
		exit 1
	fi

	log_step "Watching CI run $run_id (this can take several minutes)..."
	if ! gh run watch --repo versatiles-org/versatiles-rs --exit-status "$run_id"; then
		log_error "CI failed on $short_sha — fix forward on dev and re-run the release script"
		exit 1
	fi
	log_success "CI passed on $short_sha"
}

# Fast-forward 'main' to dev's tip. Branch protection (linear-history + green
# CI Success) is satisfied because dev's tip already has CI green.
fast_forward_main() {
	log_step "Fast-forwarding 'main' to dev's tip..."
	git push origin dev:main
	log_success "main updated"
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
			"release - Remove pre-release suffix (x.y.z-rc.N → x.y.z)"
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
				4) release_type="release"; break;;
				5) release_type="alpha"; break;;
				6) release_type="beta"; break;;
				7) release_type="rc"; break;;
				8) echo -e "${YEL}Cancelled${END}"; exit 0;;
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

	# Pre-flight: must be on dev, clean, gh available, dev up-to-date with origin,
	# and main an ancestor of dev (so the FF later is a no-rewrite operation).
	preflight_checks

	# Regenerate auto-built READMEs. If anything changed, land it as its own
	# commit before the release commit so the version-bump diff stays minimal.
	regenerate_readmes

	log_step "Running checks..."
	if ! ./scripts/check.sh; then
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

	# Update version files and create the release commit (no tag yet — we tag
	# only after CI confirms the commit is green).
	update_cargo_versions "$new_version"
	update_package_json_version "$new_version"

	echo ""

	create_release_commit "$new_version"

	echo ""

	# Push dev so CI runs on the new commit, wait for it to pass, then move
	# main forward. Branch protection on main requires the tip to have a green
	# 'CI Success' check; this sequencing satisfies that.
	push_dev_and_wait_for_ci
	fast_forward_main

	# Tag only the green commit and push the tag separately so the v-tag
	# ruleset only fires once we're confident in the release.
	echo ""
	create_release_tag "$new_version"
	log_step "Pushing tag v$new_version..."
	git push origin "v$new_version"

	echo ""
	echo -e "${GRE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
	echo -e "${GRE}✓ Release v$new_version completed successfully!${END}"
	echo -e "${GRE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
}

main "$@"
