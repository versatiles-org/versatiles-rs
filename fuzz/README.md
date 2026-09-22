# Fuzz targets

Four parsers that consume untrusted bytes, driven by [`cargo-fuzz`].

The 2026-09 security audit found nine input-handling bugs by hand. Four of them
— an out-of-bounds tile index, unbounded directory recursion, a `run_length`
costing one byte and asking for gigabytes, and a protobuf length allocating
before it was checked — are the kind a fuzzer reaches in seconds. These targets
exist so the next one is found that way instead.

## Running

```sh
cargo install cargo-fuzz          # once
cargo +nightly fuzz run vector_tile
```

If `cargo-fuzz` came from a prebuilt binary rather than `cargo install` — which
is what most CI setups do — pass the target explicitly:

```sh
cargo +nightly fuzz run vector_tile --target "$(rustc -vV | sed -n 's|^host: ||p')"
```

It defaults `--target` to the triple it was _itself_ built for, and the prebuilt
Linux binary is statically linked against musl. Without the flag it builds for
`x86_64-unknown-linux-musl`, which is usually not installed and which implies
`crt-static`, and the sanitizer refuses that combination.

Nightly is required: libFuzzer needs `-Z sanitizer`, which stable does not
expose. This is the one place in the repository that is not built on the pinned
stable toolchain.

Targets:

| Target                 | Input                              |
| ---------------------- | ---------------------------------- |
| `vector_tile`          | Mapbox Vector Tile protobuf        |
| `vpl`                  | VPL text, both parsers             |
| `pmtiles_container`    | a `.pmtiles` file, opened and read |
| `versatiles_container` | a `.versatiles` file, likewise     |

A time-boxed run, which is what a scheduled job wants:

```sh
cargo +nightly fuzz run vpl -- -max_total_time=600 -rss_limit_mb=2048
```

Set `-rss_limit_mb`. Without it an unbounded allocation is reported as a crash
only once the machine is already in trouble; with it libFuzzer stops at the
limit and writes the input that got there.

## What counts as a finding

An error return is the _expected_ outcome for almost every input — these are
parsers being handed noise. What the targets look for is a panic, an abort, a
hang, or an allocation that runs away. All four are failures a caller cannot
recover from: a stack overflow and a failed allocation abort the process
without unwinding, so neither `catch_unwind` nor the server's `CatchPanicLayer`
sees them.

Reproduce a finding with the input libFuzzer writes:

```sh
cargo +nightly fuzz run vector_tile fuzz/artifacts/vector_tile/oom-<hash>
```

Then add it as a regression test next to the code it broke — the existing ones
build their malformed input inline rather than committing a binary, so they
stay readable and survive a format change.

## Where they belong in CI

On a schedule, not per-PR. A few minutes per target finds shallow bugs; a
per-PR run finds nothing new on most PRs and slows every one of them down. The
corpus is what makes successive runs cumulative, so a scheduled job should
carry `fuzz/corpus/<target>/` between runs rather than start cold each time.

`corpus/` and `artifacts/` are not committed.

[`cargo-fuzz`]: https://rust-fuzz.github.io/book/cargo-fuzz.html
