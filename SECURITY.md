# Security Policy

## Supported versions

Only the current major release line of VersaTiles receives security fixes. Older releases are not maintained.

## Reporting a vulnerability

Please **do not** open a public issue for security reports.

Use GitHub's [private vulnerability reporting](https://github.com/versatiles-org/versatiles-rs/security/advisories/new). That notifies the maintainers privately and lets us coordinate a fix and disclosure.

If you cannot use GitHub for some reason, email `versatiles@michael-kreil.de` instead.

We aim to acknowledge reports within 7 days and to work with you on a coordinated disclosure timeline.

## Fixes and patch releases

A vulnerability in VersaTiles, or a dependency advisory that affects the released binaries, is fixed in a patch release of the current major line. For high or critical severity we aim to publish it within 7 days of the fix being known, without waiting for other changes. The fix is listed in the release notes with its advisory ID.

## Checking a deployed binary

The release binaries of the `versatiles` CLI after v5.7.0 carry their dependency list, built in with [`cargo auditable`](https://github.com/rust-secure-code/cargo-auditable). A scanner can read it from the binary and match it against advisories, so a server can be checked without knowing which release it runs:

```sh
cargo audit bin /usr/local/bin/versatiles
```

Trivy, Grype and Syft read the same data. Binaries built from source with plain `cargo build` or `cargo install` don't carry it; build them with `cargo auditable build` or `cargo auditable install` instead.

To check that a downloaded release was built by this repository's workflow:

```sh
gh attestation verify --owner versatiles-org versatiles-<platform>.tar.gz
```

## Scope

In scope:

- The `versatiles` CLI and library crates (`versatiles_core`, `versatiles_container`, `versatiles_pipeline`, `versatiles_geometry`, `versatiles_image`).
- The HTTP tile server (`versatiles serve`).
- The Node.js bindings (`versatiles_node`).
- Container readers and writers handling untrusted input (`.versatiles`, `.pmtiles`, `.mbtiles`, `.tar`, directory).

A pipeline (`.vpl`) is a program: running one you did not write is like running a script. One opened from a URL cannot read the files of the machine that runs it, because every path in it resolves to a URL on its own server. It can still make that machine fetch any `http`, `https` or `sftp` URL, including addresses on the local network. Do not build pipelines from untrusted input on a machine that can reach services which are not public.

Out of scope:

- Issues that require an already-compromised host.
- Denial of service via legitimate but very large inputs — please file as a regular issue.
- Vulnerabilities in third-party hosted services.
