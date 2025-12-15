# Release Process

## Automated Release (Recommended)

### 1. Prepare
```bash
git checkout main
git pull origin main
./scripts/sync-version.sh  # Verify versions match
```

### 2. Create Release
```bash
# Patch release (2.3.1 → 2.3.2)
./scripts/release-package.sh patch

# Minor release (2.3.1 → 2.4.0)
./scripts/release-package.sh minor

# Major release (2.3.1 → 3.0.0)
./scripts/release-package.sh major
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
- Build all platform binaries (Linux gnu/musl x64/arm64, macOS x64/arm64, Windows x64/arm64)
- Package npm binaries using NAPI-RS
- Publish to npmjs.com (main package + 8 platform-specific packages)
- Create GitHub release
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

## Manual npm Publish (Fallback)

If automated publishing fails:

```bash
cd versatiles_node

# Ensure versions match
npm version $(grep -m1 '^version = ' ../Cargo.toml | sed 's/version = "\(.*\)"/\1/') --no-git-tag-version

# Download binaries from GitHub release
TAG=v2.3.2
gh release download "$TAG" -D ../artifacts/

# Extract and rename to .node files
cd ../artifacts
for archive in *.tar.gz; do tar -xzf "$archive"; done

# Place in versatiles_node/ with correct names
# (See workflow for naming convention)

# Generate platform packages
cd ../versatiles_node
npm run artifacts

# Publish
npm login
for pkg in npm/*.tgz; do
  npm publish "$pkg" --access public
done
npm run prepublishOnly
npm publish --access public
```

## Version Strategy

- **Patch** (x.y.Z): Bug fixes, small improvements
- **Minor** (x.Y.0): New features, backward compatible
- **Major** (X.0.0): Breaking changes

We follow Semantic Versioning 2.0.0.

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
