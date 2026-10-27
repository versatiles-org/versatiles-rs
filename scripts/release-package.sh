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
VALID_KEYWORDS="patch minor major release alpha beta rc retry resume"

REPO="versatiles-org/versatiles-rs"

# How far the release got, for the hint printed if the script stops early.
# "pushed": the release commit is on origin/dev; "main": main points at it too.
RELEASE_STAGE=""

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

# If the script stops after the release commit reached origin, say so and how to
# finish: otherwise a half-done release looks like nothing happened.
on_exit() {
	local status=$?
	[ "$status" -eq 0 ] && return
	local where="origin/dev"
	[ "$RELEASE_STAGE" = "main" ] && where="origin/dev and main"
	case "$RELEASE_STAGE" in
		pushed | main)
			echo ""
			log_error "Stopped before the release was tagged (the release commit is on $where)."
			echo "  Once its CI is green, finish it with: ./scripts/release-package.sh resume"
			;;
	esac
}
trap on_exit EXIT

# On macOS, keep the machine from idle-sleeping while the script waits for CI.
# Closing the lid still sleeps it.
keep_awake() {
	if command -v caffeinate >/dev/null 2>&1; then
		caffeinate -i -w $$ &
	fi
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

	if [ "$version" = "$current_version" ]; then
		log_error "New version must be different from current version ($current_version)"
		exit 1
	fi

	# `cargo release version` without `--execute` is a dry run, and it checks both
	# things worth checking: that the version parses, and that no crate would move
	# backwards. Let it be the judge rather than re-deriving semver here.
	#
	# `2>&1`, not `2>/dev/null`: cargo-release writes this to **stderr**. Discarding
	# it meant the grep below never matched and *every* explicit version was
	# rejected as invalid semver, however well formed. `calculate_new_version`
	# redirects correctly, which is why the keyword path always worked.
	local output
	if ! output=$(cargo release version "$version" --workspace 2>&1) ||
		! grep -q "Upgrading workspace to version" <<<"$output"; then
		log_error "Cannot release version $version (current: $current_version):"
		grep -E "^error|^warning" <<<"$output" | head -3
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
	local commit="${2:-HEAD}"

	log_step "Creating release tag..."

	git tag -s "v$new_version" -m "Release v$new_version" "$commit"

	log_success "Tag created: v$new_version"
}

# Abort if the version's tag already exists (locally or on origin) — that
# would mean the release is already published and must not be repeated.
ensure_tag_absent() {
	local v="$1"

	if git rev-parse -q --verify "refs/tags/v$v" >/dev/null 2>&1; then
		log_error "Tag v$v already exists locally — this version is already released"
		exit 1
	fi

	if git ls-remote --exit-code --tags origin "v$v" >/dev/null 2>&1; then
		log_error "Tag v$v already exists on origin — this version is already released"
		exit 1
	fi
}

# Retry mode: the version's tag may already exist because the release was
# tagged but its release workflow failed. That is expected — the tag will be
# moved to the new release commit later. Abort only if the release actually
# finished, detected via a finalized (non-draft) GitHub release.
check_retry_tag() {
	local v="$1"

	if ! git rev-parse -q --verify "refs/tags/v$v" >/dev/null 2>&1 &&
		! git ls-remote --exit-code --tags origin "v$v" >/dev/null 2>&1; then
		return 0 # no tag yet — it will simply be created
	fi

	local is_draft
	is_draft=$(gh release view "v$v" --repo "$REPO" \
		--json isDraft --jq '.isDraft' 2>/dev/null || echo "missing")
	if [ "$is_draft" = "false" ]; then
		log_error "v$v is already published (its GitHub release is finalized) — nothing to retry"
		exit 1
	fi

	log_step "Tag v$v already exists from a failed attempt — it will be moved to the new release commit"
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

	if ! command -v cargo-deny >/dev/null 2>&1; then
		log_error "Required: cargo-deny for the pre-release checks (brew install cargo-deny)"
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
	RELEASE_STAGE="pushed"

	log_step "Locating CI run for $short_sha..."
	local run_id=""
	for _ in 1 2 3 4 5 6; do
		run_id=$(gh run list \
			--repo "$REPO" \
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
	if ! wait_for_run "$run_id"; then
		RELEASE_STAGE=""
		log_error "CI failed on $short_sha — fix forward on dev, then: ./scripts/release-package.sh retry"
		exit 1
	fi
	log_success "CI passed on $short_sha"
}

# Wait until a workflow run completes; succeed only if it succeeded.
#
# `gh run watch` exits non-zero for its own trouble too (a network drop, an API
# error), and reading that as a red CI run stopped a release whose CI was green.
# So the run's conclusion is asked for separately, and an interrupted watch is
# resumed. Gives up after ten consecutive gh errors.
wait_for_run() {
	local run_id="$1"
	local errors=0
	local state

	while true; do
		if gh run watch --compact --repo "$REPO" "$run_id"; then
			errors=0
		else
			errors=$((errors + 1))
		fi

		if ! state=$(gh run view "$run_id" --repo "$REPO" \
			--json status,conclusion --jq '"\(.status) \(.conclusion)"' 2>/dev/null); then
			errors=$((errors + 1))
			state="unknown"
		fi

		case "$state" in
			"completed success") return 0 ;;
			completed\ *)
				log_error "CI run $run_id finished: ${state#completed }"
				return 1
				;;
		esac

		if [ "$errors" -ge 10 ]; then
			log_error "Could not follow CI run $run_id (gh kept failing): https://github.com/$REPO/actions/runs/$run_id"
			exit 1
		fi
		log_step "Watching CI run $run_id was interrupted (run is $state) — watching again..."
		sleep 15
	done
}

# Whether the "CI Success" check passed on a commit.
ci_succeeded() {
	local conclusion
	conclusion=$(gh api "repos/$REPO/commits/$1/check-runs?check_name=CI%20Success" \
		--jq '.check_runs[0].conclusion // "missing"')
	[ "$conclusion" = "success" ]
}

# Finish a release that stopped after its release commit reached origin/dev:
# fast-forward main to that commit and tag it. Nothing is rebuilt or pushed to
# dev, and later dev commits stay out of the release, because the commit that
# gets tagged is the one CI checked.
resume_release() {
	local v
	v=$(get_current_version)

	local sha
	sha=$(git log origin/dev --format='%H%x09%s' |
		awk -F'\t' -v s="release: v$v" '$2 == s { print $1; exit }')
	if [ -z "$sha" ]; then
		log_error "No 'release: v$v' commit on origin/dev — nothing to resume"
		exit 1
	fi
	local short_sha
	short_sha=$(git rev-parse --short "$sha")
	log_step "Resuming v$v at release commit $short_sha"

	if git ls-remote --exit-code --tags origin "v$v" >/dev/null 2>&1; then
		log_error "Tag v$v is already on origin — the release workflow has been started"
		echo "  If it failed, fix forward on dev and run: ./scripts/release-package.sh retry"
		exit 1
	fi
	if git rev-parse -q --verify "refs/tags/v$v" >/dev/null 2>&1 &&
		[ "$(git rev-parse "v$v^{commit}")" != "$sha" ]; then
		log_error "Local tag v$v points elsewhere than $short_sha — delete it first: git tag -d v$v"
		exit 1
	fi

	if ! ci_succeeded "$sha"; then
		log_error "'CI Success' has not passed on $short_sha"
		echo "  Wait for its CI run, or fix forward on dev and run: ./scripts/release-package.sh retry"
		exit 1
	fi
	log_success "CI passed on $short_sha"
	RELEASE_STAGE="pushed"

	if git merge-base --is-ancestor "$sha" origin/main; then
		log_success "main already contains $short_sha"
	elif git merge-base --is-ancestor origin/main "$sha"; then
		log_step "Fast-forwarding 'main' to $short_sha..."
		git push origin "$sha:refs/heads/main"
		log_success "main updated"
	else
		log_error "'origin/main' has commits $short_sha does not — cannot fast-forward"
		exit 1
	fi
	RELEASE_STAGE="main"

	echo ""
	if ! git rev-parse -q --verify "refs/tags/v$v" >/dev/null 2>&1; then
		create_release_tag "$v" "$sha"
	fi
	log_step "Pushing tag v$v..."
	git push origin "v$v"
	RELEASE_STAGE=""

	echo ""
	echo -e "${GRE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
	echo -e "${GRE}✓ Release v$v completed successfully!${END}"
	echo -e "${GRE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
}

# Fast-forward 'main' to dev's tip. Branch protection (linear-history + green
# CI Success) is satisfied because dev's tip already has CI green.
fast_forward_main() {
	log_step "Fast-forwarding 'main' to dev's tip..."
	git push origin dev:main
	RELEASE_STAGE="main"
	log_success "main updated"
}

# Main function
main() {
	local version_arg="$1"
	local release_type=""
	local new_version=""
	local is_specific_version=false
	local is_retry=false

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
			"retry   - Re-release the current version unchanged (e.g. after a failed CI run)"
			"resume  - Finish a release that stopped after its green release commit was pushed"
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
				8) release_type="retry"; break;;
				9) release_type="resume"; break;;
				10) echo -e "${YEL}Cancelled${END}"; exit 0;;
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

	# "retry" re-runs the release pipeline for the version already in Cargo.toml
	# (no version bump, no release commit) — used to recover a release whose CI
	# run failed after the version-bump commit had already landed.
	if [ "$release_type" = "retry" ]; then
		is_retry=true
	fi

	keep_awake

	# Pre-flight: must be on dev, clean, gh available, dev up-to-date with origin,
	# and main an ancestor of dev (so the FF later is a no-rewrite operation).
	preflight_checks

	if [ "$release_type" = "resume" ]; then
		resume_release
		return
	fi

	# Regenerate auto-built READMEs. If anything changed, land it as its own
	# commit before the release commit so the version-bump diff stays minimal.
	regenerate_readmes

	# Only the two quick checks most likely to turn the release CI red: it
	# checks everything scripts/check.sh does and more, and the release waits
	# for it before main and the tag move. Format, because `cargo fmt-check` is
	# the cheapest way to fail; deny, because a new RustSec advisory reddens CI
	# without any change in the repository.
	log_step "Running checks..."
	if ! cargo fmt-check; then
		log_error "Format check failed! Run: cargo fmt-all"
		exit 1
	fi
	if ! cargo deny check; then
		log_error "cargo deny failed!"
		exit 1
	fi
	log_success "Checks passed"

	# Get current version
	local current_version
	current_version=$(get_current_version)
	echo ""
	echo -e "${BLU}Current version: $current_version${END}"

	# Calculate or validate new version
	if [ "$is_retry" = true ]; then
		# Retry: reuse the version already committed to Cargo.toml as-is.
		new_version="$current_version"
		log_step "Retry mode: re-releasing current version $new_version (no version bump)"
	elif [ "$is_specific_version" = true ]; then
		validate_specific_version "$new_version" "$current_version"
	else
		log_step "Calculating new version..."
		new_version=$(calculate_new_version "$release_type")
		log_success "Calculated new version: $new_version"
	fi

	# Non-retry: the tag must not exist yet. Retry: the tag may exist (a failed
	# attempt) and will be moved; only abort if the release actually finished.
	if [ "$is_retry" = true ]; then
		check_retry_tag "$new_version"
	else
		ensure_tag_absent "$new_version"
	fi

	echo ""
	echo -e "${BLU}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
	echo -e "${BLU}Releasing version: $new_version${END}"
	echo -e "${BLU}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
	echo ""

	# Update version files and create the release commit (no tag yet — we tag
	# only after CI confirms the commit is green). Skipped on retry: the
	# version is already in Cargo.toml and was committed by the prior attempt.
	if [ "$is_retry" = true ]; then
		log_step "Retry mode: skipping version bump and release commit"
	else
		update_cargo_versions "$new_version"
		update_package_json_version "$new_version"

		echo ""

		create_release_commit "$new_version"
	fi

	echo ""

	# Push dev so CI runs on the new commit, wait for it to pass, then move
	# main forward. Branch protection on main requires the tip to have a green
	# 'CI Success' check; this sequencing satisfies that.
	push_dev_and_wait_for_ci
	fast_forward_main

	# Tag only the green commit and push the tag separately so the v-tag
	# ruleset only fires once we're confident in the release.
	echo ""
	# Retry: drop the existing tag (origin + local) so it can be re-created on
	# the freshly-verified commit — pushing it again retriggers the release
	# workflow, now with whatever fixes have landed since the failed attempt.
	if [ "$is_retry" = true ]; then
		if git ls-remote --exit-code --tags origin "v$new_version" >/dev/null 2>&1; then
			log_step "Removing old tag v$new_version from origin..."
			git push origin ":refs/tags/v$new_version"
		fi
		git tag -d "v$new_version" >/dev/null 2>&1 || true
	fi
	create_release_tag "$new_version"
	log_step "Pushing tag v$new_version..."
	git push origin "v$new_version"
	RELEASE_STAGE=""

	echo ""
	echo -e "${GRE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
	echo -e "${GRE}✓ Release v$new_version completed successfully!${END}"
	echo -e "${GRE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
}

main "$@"
