# Security Policy

## Supported versions

Only the current major release line of VersaTiles receives security fixes. Older releases are not maintained.

## Reporting a vulnerability

Please **do not** open a public issue for security reports.

Use GitHub's [private vulnerability reporting](https://github.com/versatiles-org/versatiles-rs/security/advisories/new). That notifies the maintainers privately and lets us coordinate a fix and disclosure.

If you cannot use GitHub for some reason, email `versatiles@michael-kreil.de` instead.

We aim to acknowledge reports within 7 days and to work with you on a coordinated disclosure timeline.

## Scope

In scope:

- The `versatiles` CLI and library crates (`versatiles_core`, `versatiles_container`, `versatiles_pipeline`, `versatiles_geometry`, `versatiles_image`).
- The HTTP tile server (`versatiles serve`).
- The Node.js bindings (`versatiles_node`).
- Container readers and writers handling untrusted input (`.versatiles`, `.pmtiles`, `.mbtiles`, `.tar`, directory).

Out of scope:

- Issues that require an already-compromised host.
- Denial of service via legitimate but very large inputs — please file as a regular issue.
- Vulnerabilities in third-party hosted services.
