#!/usr/bin/env bash
cd "$(dirname "$0")/.."

set -e

# Colors
RED="\033[1;31m"
GRE="\033[1;32m"
YEL="\033[1;33m"
BLU="\033[1;34m"
MAG="\033[1;35m"
CYA="\033[1;36m"
END="\033[0m"

# Help message
show_help() {
	cat <<EOF
${BLU}Usage:${END} $0 [OPTIONS]

${BLU}Description:${END}
  Audit unused dependencies in the workspace using cargo-udeps.
  Runs multiple checks with different feature combinations.

${BLU}Options:${END}
  -h, --help          Show this help message
  -s, --silent        Suppress cargo output (show only summary)
  -f, --fail-fast     Exit on first failure
  -t, --timing        Show timing information for each check
  -c, --check NAME    Run only specific check (see checks below)

${BLU}Available Checks:${END}
  binary              Check unused deps for binary targets
  lib-minimal         Check library with no default features
  lib-default         Check library with default features (cli + server)
  lib-cli             Check library with CLI features only
  lib-server          Check library with server features only
  lib-test            Check library with test features
  lib-gdal            Check library with GDAL features
  lib-bindgen         Check library with GDAL + bindgen features
  lib-all             Check library with all features
  all                 Run all checks (default)

${BLU}Examples:${END}
  $0                      # Run all checks
  $0 -t                   # Run with timing information
  $0 -s                   # Silent mode
  $0 -c binary            # Check only binary targets
  $0 -f -t                # Fail fast with timing

${BLU}Requirements:${END}
  - Nightly Rust toolchain
  - cargo-udeps (install with: cargo install cargo-udeps)

EOF
}

# Parse arguments
SILENT=false
FAIL_FAST=false
SHOW_TIMING=false
SPECIFIC_CHECK=""

while [[ $# -gt 0 ]]; do
	case $1 in
		-h|--help)
			show_help
			exit 0
			;;
		-s|--silent)
			SILENT=true
			shift
			;;
		-f|--fail-fast)
			FAIL_FAST=true
			shift
			;;
		-t|--timing)
			SHOW_TIMING=true
			shift
			;;
		-c|--check)
			SPECIFIC_CHECK="$2"
			shift 2
			;;
		*)
			echo -e "${RED}Unknown option: $1${END}"
			echo "Use --help for usage information"
			exit 1
			;;
	esac
done

# Check if nightly toolchain is installed
echo -e "${BLU}Checking for nightly toolchain...${END}"
if ! rustup toolchain list | grep -q nightly; then
	echo -e "${YEL}Nightly toolchain not found${END}"
	echo -e "${BLU}Installing nightly toolchain...${END}"
	rustup toolchain install nightly
fi

# Check if cargo-udeps is installed
if ! cargo +nightly udeps --version &>/dev/null; then
	echo -e "${RED}cargo-udeps not found${END}"
	echo -e "${BLU}Install with: cargo install cargo-udeps${END}"
	exit 1
fi

echo ""

# Track results
declare -a RESULTS
declare -a TIMINGS
TOTAL_CHECKS=0
PASSED_CHECKS=0
FAILED_CHECKS=0

# Run a single check
run_check() {
	local name="$1"
	local description="$2"
	shift 2
	local args=("$@")

	# Skip if specific check requested and this isn't it
	if [ -n "$SPECIFIC_CHECK" ] && [ "$SPECIFIC_CHECK" != "$name" ] && [ "$SPECIFIC_CHECK" != "all" ]; then
		return 0
	fi

	TOTAL_CHECKS=$((TOTAL_CHECKS + 1))

	echo -e "${CYA}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
	echo -e "${YEL}Check $TOTAL_CHECKS: $description${END}"
	echo -e "${CYA}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"

	# Build command
	local cmd="cargo +nightly udeps"
	if [ "$SILENT" = true ]; then
		cmd="$cmd -q"
	fi
	cmd="$cmd ${args[*]}"

	echo -e "${MAG}Command:${END} $cmd"
	echo ""

	# Run with timing
	local start_time
	local end_time
	local duration

	if [ "$SHOW_TIMING" = true ]; then
		start_time=$(date +%s)
	fi

	# Execute command
	local output
	local exit_code=0

	if output=$(eval "$cmd" 2>&1); then
		echo -e "${GRE}✓ PASSED${END}"
		PASSED_CHECKS=$((PASSED_CHECKS + 1))
		RESULTS+=("${GRE}✓${END} $description")
	else
		exit_code=$?
		echo "$output"
		echo ""
		echo -e "${RED}✗ FAILED${END}"
		FAILED_CHECKS=$((FAILED_CHECKS + 1))
		RESULTS+=("${RED}✗${END} $description")

		if [ "$FAIL_FAST" = true ]; then
			echo ""
			echo -e "${RED}Exiting due to --fail-fast${END}"
			exit $exit_code
		fi
	fi

	if [ "$SHOW_TIMING" = true ]; then
		end_time=$(date +%s)
		duration=$((end_time - start_time))
		TIMINGS+=("$duration")
		echo -e "${BLU}Duration: ${duration}s${END}"
	fi

	echo ""
}

# Run checks
echo -e "${BLU}Starting dependency audit...${END}"
echo ""

run_check "binary" \
	"Unused dependencies for binary targets" \
	--bins

run_check "lib-minimal" \
	"Unused dependencies for library (minimal, no default features)" \
	--lib --workspace --no-default-features

run_check "lib-default" \
	"Unused dependencies for library (default features: cli + server)" \
	--lib --workspace

run_check "lib-cli" \
	"Unused dependencies for library (CLI features only)" \
	--lib --workspace --no-default-features --features cli --exclude versatiles

run_check "lib-server" \
	"Unused dependencies for library (server features only)" \
	--lib --workspace --no-default-features --features server --exclude versatiles

run_check "lib-test" \
	"Unused dependencies for library (test features)" \
	--lib --workspace --no-default-features --features test

run_check "lib-gdal" \
	"Unused dependencies for library (GDAL features)" \
	--lib --workspace --no-default-features --features gdal

run_check "lib-bindgen" \
	"Unused dependencies for library (GDAL with bindgen features)" \
	--lib --workspace --no-default-features --features gdal,bindgen

run_check "lib-all" \
	"Unused dependencies for library (all features)" \
	--lib --workspace --all-features --exclude versatiles --exclude versatiles_core

# Print summary
echo -e "${CYA}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
echo -e "${BLU}Summary${END}"
echo -e "${CYA}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${END}"
echo ""

for i in "${!RESULTS[@]}"; do
	echo -e "  ${RESULTS[$i]}"
done

echo ""
echo -e "${BLU}Total checks:${END} $TOTAL_CHECKS"
echo -e "${GRE}Passed:${END}       $PASSED_CHECKS"

if [ $FAILED_CHECKS -gt 0 ]; then
	echo -e "${RED}Failed:${END}       $FAILED_CHECKS"
fi

if [ "$SHOW_TIMING" = true ] && [ ${#TIMINGS[@]} -gt 0 ]; then
	local total_time=0
	for t in "${TIMINGS[@]}"; do
		total_time=$((total_time + t))
	done
	echo ""
	echo -e "${BLU}Total time:${END}    ${total_time}s"
fi

echo ""

# Exit with error if any checks failed
if [ $FAILED_CHECKS -gt 0 ]; then
	echo -e "${RED}Some checks failed!${END}"
	exit 1
else
	echo -e "${GRE}All checks passed!${END}"
	exit 0
fi
