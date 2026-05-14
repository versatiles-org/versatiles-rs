# Contributing to VersaTiles

Thanks for your interest! Contributions of any size — bug reports, fixes, features, docs — are welcome.

For security issues, follow [SECURITY.md](./SECURITY.md) instead of opening a public issue.

## Setting up

Prerequisites and the full development workflow are documented in the [Development section of the README](./README.md#development). Quick version:

```bash
git clone https://github.com/versatiles-org/versatiles-rs
cd versatiles-rs
cargo test
```

If your change touches GDAL-backed operations, also run `./scripts/install-gdal.sh` once.

## Before you push

Run the same checks CI will run:

```bash
./scripts/check.sh
```

That wraps `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, and the Markdown and Node.js linters. Push only after it passes locally.

## Commit and PR conventions

- Use [conventional commit](https://www.conventionalcommits.org/) prefixes — `feat:`, `fix:`, `refactor:`, `docs:`, `ci:`, `test:`, `chore:`. Look at recent commits for examples.
- One concern per PR. Keep diffs focused; unrelated cleanups belong in a separate PR.
- Add or update tests for any code change that can be tested.
- Update `README.md` and `versatiles_pipeline/README.md` if you add user-facing operations, CLI flags, or behaviour.
- The `main` branch requires linear history; PRs are merged via rebase only — no squash, no merge commits. Keep your branch up to date with `main` before requesting a final review.

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](./LICENSE) that covers the project.

## Questions

If you're unsure whether something is in scope or a good idea, open an issue first to discuss before writing code. That is almost always cheaper than rework afterwards.
