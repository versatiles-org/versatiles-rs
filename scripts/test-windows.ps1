#!/usr/bin/env pwsh
# Robust, strict, and feature-matrixed Rust checks on Windows

# --- Shell strictness
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$ProgressPreference = 'SilentlyContinue'

# --- Repo root
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location -Path $root

# --- Env
#$env:RUST_BACKTRACE = '1'
$Script:ForwardArgs = $args

function Invoke-Step {
  param(
    [Parameter(Mandatory)] [string] $Name,
    [Parameter(Mandatory)] [scriptblock] $Action
  )
  Write-Host "=== $Name ==="
  & $Action
}

# Format (fail on diff)
Invoke-Step "Format check (rustfmt)" {
  cargo fmt -- --check
}

# Clippy: all targets
Invoke-Step "Clippy (all targets)" {
  cargo clippy --workspace --all-targets -- -D warnings @args
}

# Tests: all targets
Invoke-Step "Tests (all targets)" {
  cargo test --workspace --all-targets @args
}

# Doctests (all-features)
Invoke-Step "Doctests (all-features)" {
  cargo test --workspace --doc @args
}