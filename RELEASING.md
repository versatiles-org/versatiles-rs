# Release Process

## Automated Release (Recommended)

### 1. Prepare

```bash
git checkout main
git pull origin main
./scripts/sync-version.sh  # Verify versions match
```

### 2. Create Release

You can either provide the release type as an argument, or run the script without arguments for an interactive menu.

#### Interactive Mode

```bash
# Run without arguments to see interactive menu
./scripts/release-package.sh

# You'll see:
# Select release type:
#
# 1) patch   - Bug fixes, small improvements (x.y.Z)
# 2) minor   - New features, backward compatible (x.Y.0)
# 3) major   - Breaking changes (X.0.0)
# 4) alpha   - Early development, unstable API (x.y.z-alpha.N)
# 5) beta    - Feature complete, testing phase (x.y.z-beta.N)
# 6) rc      - Release candidate, final testing (x.y.z-rc.N)
# 7) dev     - Daily builds, experimental features (x.y.z-dev.N)
# 8) Cancel
#
# Enter selection number:
```

#### Command-Line Mode

##### Stable Releases

```bash
# Patch release (2.3.1 → 2.3.2)
./scripts/release-package.sh patch

# Minor release (2.3.1 → 2.4.0)
./scripts/release-package.sh minor

# Major release (2.3.1 → 3.0.0)
./scripts/release-package.sh major
```

##### Prerelease Versions

**Alpha releases** (early development, unstable):

```bash
# First alpha from 2.3.1 → 2.4.0-alpha.1
./scripts/release-package.sh alpha

# Increment alpha → 2.4.0-alpha.2
./scripts/release-package.sh alpha
```

**Beta releases** (feature complete, testing):

```bash
# From alpha to beta: 2.4.0-alpha.2 → 2.4.0-beta.1
./scripts/release-package.sh beta

# Increment beta → 2.4.0-beta.2
./scripts/release-package.sh beta
```

**Release candidates** (final testing):

```bash
# From beta to rc: 2.4.0-beta.2 → 2.4.0-rc.1
./scripts/release-package.sh rc

# Increment rc → 2.4.0-rc.2
./scripts/release-package.sh rc
```

**Dev releases** (daily builds, experiments):

```bash
# Create dev release: 2.3.1 → 2.4.0-dev.1
./scripts/release-package.sh dev

# Increment dev → 2.4.0-dev.2
./scripts/release-package.sh dev
```

**Graduating to stable**:

```bash
# From rc to stable: 2.4.0-rc.1 → 2.4.0
./scripts/release-package.sh patch
```

This script will:

- Sync versions between Cargo.toml and package.json
- Run tests
- Execute cargo-release (updates Cargo.toml, creates commit and tag)
- Update package.json
- Amend commit to include package.json

### 3. Push

```bash
git push origin main --follow-tags
```

This triggers GitHub Actions which will:

- Validate version synchronization
- Build CLI binaries for 8 platforms (Linux gnu/musl x64/arm64, macOS x64/arm64, Windows x64/arm64)
- Build NAPI-RS bindings for Node.js (8 platform-specific .node files)
- Upload CLI binaries to GitHub release
- Package NAPI bindings using NAPI-RS
- Publish to npmjs.com (main package + 8 platform-specific packages)
- Trigger Docker and Homebrew workflows

### 4. Verify

```bash
# Check npm
npm view @versatiles/versatiles-rs

# Check platform packages
npm view @versatiles/versatiles-rs-darwin-arm64
npm view @versatiles/versatiles-rs-linux-x64-gnu

# Check GitHub
gh release view v2.3.2
```

## Prerelease Publishing Behavior

When you publish a prerelease version:

### npm Distribution

- **Alpha**: Published with `--tag alpha`
  - Install: `npm install @versatiles/versatiles-rs@alpha`
- **Beta**: Published with `--tag beta`
  - Install: `npm install @versatiles/versatiles-rs@beta`
- **RC**: Published with `--tag rc`
  - Install: `npm install @versatiles/versatiles-rs@rc`
- **Dev**: Published with `--tag dev`
  - Install: `npm install @versatiles/versatiles-rs@dev`
- **Stable**: Published with `--tag latest` (default)
  - Install: `npm install @versatiles/versatiles-rs`

### GitHub Release

- Marked as "Pre-release" (not "Latest")
- CLI binaries available for download
- Does not update "latest" release badge

### Docker & Homebrew

- **Skipped for all prereleases**
- Only triggered on stable releases

### Version Examples

```bash
# List all versions including prereleases
npm view @versatiles/versatiles-rs versions

# Get current alpha version
npm view @versatiles/versatiles-rs@alpha version

# Install specific prerelease
npm install @versatiles/versatiles-rs@2.4.0-beta.2
```

## Manual npm Publish (Fallback)

If automated publishing fails, you'll need to rebuild the NAPI bindings locally for each platform:

```bash
cd versatiles_node

# Ensure versions match
npm version $(grep -m1 '^version = ' ../Cargo.toml | sed 's/version = "\(.*\)"/\1/') --no-git-tag-version

# Build for your current platform (example for macOS ARM64)
npm run build

# For cross-platform builds, you'll need:
# - Linux: Docker or cross-compilation toolchain
# - macOS: Access to both x64 and ARM64 machines
# - Windows: Windows build environment or cross-compilation

# Once all .node files are in versatiles_node/, generate platform packages
npm run artifacts

# Publish
npm login
for pkg in npm/*.tgz; do
  npm publish "$pkg" --access public
done
npm run prepublishOnly
npm publish --access public
```

**Note:** Manual cross-platform building is complex. If only some platforms failed, you can download the successful `.node` files from the workflow artifacts in GitHub Actions.

## Version Strategy

- **Patch** (x.y.Z): Bug fixes, small improvements
- **Minor** (x.Y.0): New features, backward compatible
- **Major** (X.0.0): Breaking changes
- **Alpha** (x.y.z-alpha.N): Early development, unstable API
- **Beta** (x.y.z-beta.N): Feature complete, testing phase
- **RC** (x.y.z-rc.N): Release candidate, final testing
- **Dev** (x.y.z-dev.N): Daily builds, experimental features

We follow Semantic Versioning 2.0.0 with prerelease identifiers.

### Typical Prerelease Progression

```
2.3.1 (stable)
  ↓ alpha
2.4.0-alpha.1
  ↓ alpha (increment)
2.4.0-alpha.2
  ↓ beta (graduate)
2.4.0-beta.1
  ↓ rc (graduate)
2.4.0-rc.1
  ↓ patch (graduate to stable)
2.4.0 (stable)
```

## Troubleshooting

### Version Mismatch Error

If pre-flight checks fail with version mismatch:

```bash
./scripts/sync-version.sh --fix
git add versatiles_node/package.json
git commit -m "chore: sync package.json version"
```

### npm Publish Failed

Check what was published:

```bash
npm view @versatiles/versatiles-rs versions
npm view @versatiles/versatiles-rs-darwin-arm64 versions
```

Re-publish missing packages manually (see Manual npm Publish section above).

### Rollback a Release

1. **Delete tag**:

   ```bash
   git push origin :refs/tags/vX.Y.Z
   git tag -d vX.Y.Z
   ```

2. **Deprecate npm packages** (can't unpublish after 72 hours):

   ```bash
   npm deprecate @versatiles/versatiles-rs@X.Y.Z "This version was incorrectly published and should not be used"
   ```

## Configuration

### GitHub Secrets Required

- `NPM_TOKEN`: Automation token from npmjs.com with publish permissions to @versatiles scope

### Local Tools Required

- cargo-release: `cargo install cargo-release`
- GitHub CLI: `brew install gh` or `apt install gh`
- Node.js >= 16

## Reference

- GitHub Actions workflow: `.github/workflows/release.yml`
- Version sync script: `scripts/sync-version.sh`
- Local release script: `scripts/release-package.sh`
