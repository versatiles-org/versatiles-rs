#!/usr/bin/env bash

# Binary Size Analysis Script for VersaTiles
# Analyzes the size of the release binary, breaking down contributions by crates and dependencies

set -euo pipefail

# Configuration
BINARY_NAME="versatiles"
BINARY_PATH="target/release/${BINARY_NAME}"
BASELINE_DIR=".size-baselines"

# Default values
CLEAN_BUILD=false
CRATES_ONLY=false
FILTER_PATTERN=""
FEATURES=""
COMPARE_BASELINE=""
SAVE_BASELINE_NAME=""
WORKSPACE_ONLY=false
DEPS_ONLY=false

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Helper functions
log_info() {
    echo -e "${BLUE}==>${NC} $1"
}

log_success() {
    echo -e "${GREEN}✓${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}⚠${NC} $1"
}

log_error() {
    echo -e "${RED}✗${NC} $1" >&2
}

show_help() {
    cat << EOF
Binary Size Analysis Script for VersaTiles

Usage: $(basename "$0") [OPTIONS]

Options:
  --clean                Clean build before analysis
  --crates-only          Show only crate-level breakdown
  --filter PATTERN       Filter results by pattern
  --features FEATURES    Comma-separated features to enable
  --compare BASELINE     Compare with saved baseline
  --save-baseline NAME   Save current analysis as baseline
  --workspace-only       Show only workspace crates
  --deps-only            Show only external dependencies
  --help                 Show this help message

Examples:
  # Quick crate-level view
  $(basename "$0") --crates-only

  # Clean build and analyze
  $(basename "$0") --clean

  # Compare with baseline
  $(basename "$0") --compare before-optimization

  # Show only external dependencies
  $(basename "$0") --deps-only

EOF
}

# Parse command-line arguments
parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            --clean)
                CLEAN_BUILD=true
                shift
                ;;
            --crates-only)
                CRATES_ONLY=true
                shift
                ;;
            --filter)
                FILTER_PATTERN="$2"
                shift 2
                ;;
            --features)
                FEATURES="$2"
                shift 2
                ;;
            --compare)
                COMPARE_BASELINE="$2"
                shift 2
                ;;
            --save-baseline)
                SAVE_BASELINE_NAME="$2"
                shift 2
                ;;
            --workspace-only)
                WORKSPACE_ONLY=true
                shift
                ;;
            --deps-only)
                DEPS_ONLY=true
                shift
                ;;
            --help)
                show_help
                exit 0
                ;;
            *)
                log_error "Unknown option: $1"
                show_help
                exit 1
                ;;
        esac
    done
}

# Check if required tools are installed
check_dependencies() {
    local missing=()

    if ! command -v cargo-bloat &> /dev/null; then
        missing+=("cargo-bloat")
    fi

    if [[ ${#missing[@]} -gt 0 ]]; then
        log_error "Missing required dependencies: ${missing[*]}"
        echo ""
        read -p "Install missing dependencies? (y/n) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            for dep in "${missing[@]}"; do
                log_info "Installing $dep..."
                cargo install "$dep"
            done
            log_success "Dependencies installed"
        else
            log_error "Cannot proceed without required tools"
            exit 1
        fi
    fi
}

# Build the release binary
build_binary() {
    if [[ "$CLEAN_BUILD" == true ]]; then
        log_info "Cleaning previous build..."
        cargo clean
    fi

    log_info "Building release binary..."

    local build_cmd="cargo build --release"
    if [[ -n "$FEATURES" ]]; then
        build_cmd="$build_cmd --features $FEATURES"
        log_info "Building with features: $FEATURES"
    fi

    if ! $build_cmd; then
        log_error "Build failed"
        exit 1
    fi

    if [[ ! -f "$BINARY_PATH" ]]; then
        log_error "Binary not found at $BINARY_PATH"
        exit 1
    fi

    log_success "Build complete"
}

# Get binary size
get_binary_size() {
    if [[ -f "$BINARY_PATH" ]]; then
        # macOS uses stat -f%z, Linux uses stat -c%s
        stat -f%z "$BINARY_PATH" 2>/dev/null || stat -c%s "$BINARY_PATH" 2>/dev/null
    else
        echo "0"
    fi
}

# Format size in human-readable format
format_size() {
    local bytes=$1
    if (( bytes >= 1048576 )); then
        printf "%.2f MB" "$(echo "scale=2; $bytes / 1048576" | bc)"
    elif (( bytes >= 1024 )); then
        printf "%.2f KB" "$(echo "scale=2; $bytes / 1024" | bc)"
    else
        printf "%d bytes" "$bytes"
    fi
}

# Generate summary
generate_summary() {
    local size
    size=$(get_binary_size)
    local size_formatted
    size_formatted=$(format_size "$size")

    echo ""
    echo "=== Summary ==="
    echo "Binary: $BINARY_PATH"
    echo "Total size: $size_formatted ($size bytes)"
    echo "Features: ${FEATURES:-default}"
    echo "Build date: $(date '+%Y-%m-%d %H:%M:%S')"
    echo ""
}

# Analyze by crates
analyze_by_crates() {
    echo ""
    echo "=== Size Breakdown by Crate ==="
    echo ""

    local cmd="cargo bloat --release --crates -n 100"

    if [[ -n "$FILTER_PATTERN" ]]; then
        $cmd | grep "$FILTER_PATTERN"
    else
        $cmd
    fi
}

# Analyze workspace crates only
analyze_workspace_crates() {
    echo ""
    echo "=== Workspace Crates Only ==="
    echo ""
    cargo bloat --release --crates -n 100 | grep -E "(versatiles_|^File)"
}

# Analyze external dependencies
analyze_external_deps() {
    echo ""
    echo "=== External Dependencies ==="
    echo ""
    cargo bloat --release --crates -n 100 | grep -v "versatiles_" | grep -v "^std"
}

# Save baseline
save_baseline() {
    local name="$1"
    mkdir -p "$BASELINE_DIR"

    local baseline_file="$BASELINE_DIR/$name.txt"

    log_info "Saving baseline '$name'..."

    {
        generate_summary
        cargo bloat --release --crates -n 1000
    } > "$baseline_file"

    log_success "Baseline saved to $baseline_file"
}

# Compare with baseline
compare_with_baseline() {
    local baseline_name="$1"
    local baseline_file="$BASELINE_DIR/$baseline_name.txt"

    if [[ ! -f "$baseline_file" ]]; then
        log_error "Baseline '$baseline_name' not found at $baseline_file"
        exit 1
    fi

    log_info "Comparing with baseline '$baseline_name'..."

    local current_file="/tmp/current-${BINARY_NAME}-size.txt"
    {
        generate_summary
        cargo bloat --release --crates -n 1000
    } > "$current_file"

    echo ""
    echo "=== Comparison with Baseline '$baseline_name' ==="
    echo ""

    # Show diff (unified format)
    diff -u "$baseline_file" "$current_file" || true

    rm -f "$current_file"
}

# Main analysis function
main_analysis() {
    # Generate summary
    generate_summary

    # Handle specific analysis modes
    if [[ "$WORKSPACE_ONLY" == true ]]; then
        analyze_workspace_crates
        return
    fi

    if [[ "$DEPS_ONLY" == true ]]; then
        analyze_external_deps
        return
    fi

    if [[ "$CRATES_ONLY" == true ]]; then
        analyze_by_crates
        return
    fi

    # Full analysis
    analyze_by_crates
}

# Main script execution
main() {
    parse_args "$@"

    # Check dependencies
    check_dependencies

    # Build if necessary
    if [[ ! -f "$BINARY_PATH" ]] || [[ "$CLEAN_BUILD" == true ]]; then
        build_binary
    else
        log_info "Using existing binary at $BINARY_PATH"
    fi

    # Handle baseline operations
    if [[ -n "$SAVE_BASELINE_NAME" ]]; then
        save_baseline "$SAVE_BASELINE_NAME"
        exit 0
    fi

    if [[ -n "$COMPARE_BASELINE" ]]; then
        compare_with_baseline "$COMPARE_BASELINE"
        exit 0
    fi

    main_analysis

    log_success "Analysis complete"
}

# Run main function
main "$@"
