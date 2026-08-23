# Writing rustdoc in this workspace

Every public item in every crate carries a doc comment, and
`missing_docs = "deny"` in `[workspace.lints.rust]` keeps it that way — an
undocumented public item is a compile error, not a warning someone might notice.

That rule guarantees a comment exists. It cannot guarantee the comment is worth
reading, and the two failure modes pull in opposite directions: a comment that
restates the code adds noise, while a missing convention costs a reader an hour.
This page is where the line goes.

`versatiles_pipeline/DOCSTYLE.md` is the companion for VPL operation and
parameter text, which is generated into the operation reference and has its own
mechanical test. Everything here is about ordinary rustdoc.

## The test: does it say something the signature does not?

```rust
/// The zoom level.                                   // ✗ the name already said that
pub level: u8,

/// Northmost tile row holding a tile, inclusive. XYZ scheme, so `y_min` is
/// the *northern* edge; `TileCoord::flip_y` converts to TMS.
pub y_min: u32,                                       // ✓ a convention a reader would get wrong
```

Both fields are one line of code. The first comment costs a line and returns
nothing; the second is the difference between correct and inverted output.

Before writing, ask what a reader could get _wrong_ here. If the honest answer
is "nothing", the item still needs its one line — but keep it to one line and
move on.

## What is worth saying

In rough order of how often it turns out to matter:

1. **Units and scheme.** Bytes or kilobytes; compressed or uncompressed; XYZ or
   TMS; degrees or radians; inclusive or exclusive bounds. Names almost never
   carry these and readers assume the convention they last used.
2. **What `None`, `0` or an empty collection means.** `total: 0` meaning "not
   known" and `tile_pyramid()` returning `None` meaning "nobody asked yet, not
   empty" are facts no signature carries.
3. **Which of two similar things this is.** `next_draw` and `next_emit` differ
   by 50× on purpose. Say which is which and why.
4. **What the caller must not assume.** That `get_or_compute_tile_pyramid` can
   run its closure twice under contention; that `input()` is `None` for a
   processor. Cheap to write, expensive to discover.
5. **Why the type exists at all**, on the type itself. `ArcRef::reversed` only
   makes sense once the reader knows two features share one arc.

## What is not

- **The type.** `Option<String>` is already on screen.
- **The name, reworded.** "Sets the tile format" on `set_tile_format`.
- **`(optional)`, `(required)`, `Returns …` for a function whose name is a
  noun.**
- **Filler.** "This method is used to", "in order to", "simply".
- **A second copy of prose that lives elsewhere.** Link instead — see below.

## Link, do not paraphrase

A fact written twice drifts. When something is already documented, point at it:

```rust
/// Layers without an `extent` — [`IssueKind::MissingExtent`].
pub missing_extent: u64,
```

The MVT rule behind that counter is spelled out on the enum variant, in another
crate. Restating it here would have created a second copy to keep in step.

The same applies to mirrored types. `FeatureImportArgs` has one field per field
of `FeatureImportConfig`, so each says nothing but which field it overrides:

```rust
/// Overrides [`FeatureImportConfig::polygon_simplify_px`].
pub polygon_simplify_px: Option<f32>,
```

The exception is the one field that does _not_ map across —
`point_reduction_value` lands in a different target depending on the selected
strategy — and that one earns a real sentence.

Intra-doc links are checked: `scripts/check-rust.sh` runs `cargo doc` with
`RUSTDOCFLAGS="-D warnings"`, so a link to something that no longer exists fails
the build. This catches more than typos — a broken link is often a sentence that
became false. A doc comment claiming `MOCK_BYTES_JPG` was served by
`MockReaderProfile::Jpg` was caught this way: no such variant, and the claim was
wrong, not just the link.

Use `[`Type::method`]` and let rustdoc resolve it. Add an explicit
`(path::to::Thing)` target only when the name is not in scope — and if the label
already resolves, the explicit target is a `redundant_explicit_links` error.

## Module docs earn their keep

A module's `//!` block is the only place to explain how several types relate,
and it is where a reader who arrived at one type finds the other three. Say what
the module is _for_, and what a reader would otherwise have to infer:

- `versatiles_container`'s crate docs explain why there are four IO traits, and
  which formats support which — a question no single trait can answer.
- `versatiles_core::compression` names its three codecs, the two types that
  choose between them, and the decompression cap that stops a hostile input.

Then cross-link from the types back, so it is reachable from wherever a reader
lands.

## Wording

Third person, present tense, describing what the item does rather than
addressing the reader:

- Functions: `Reads …`, `Returns …`, `Errors when …`. Not `Read the file.`
- Types and fields: a noun phrase. `The layer's name, as written in the tile.`
- `# Errors` and `# Panics` sections are not required — `missing_errors_doc` and
  `missing_panics_doc` are allowed workspace-wide — but state a surprising
  panic or a non-obvious error in the prose.

Anything that is a name goes in backticks: identifiers, paths, literals, file
extensions, option keys.

## Examples

Examples are compiled as doctests, so they cannot drift. That makes them the
strongest documentation available and the most expensive.

- Reach for one when the _shape_ of a call is the hard part — a builder, a
  multi-step setup, a type whose construction is not obvious.
- Skip it when the signature is the whole story.
- Use `no_run` when the example needs a file or a network, so it still compiles.

Example coverage is deliberately uneven across the workspace and is not
enforced.

## Checklist

- [ ] Every public item has a doc comment (enforced — it will not compile
      otherwise).
- [ ] Nothing restates the item's name or type.
- [ ] Units, schemes and bounds are stated wherever a reader could guess wrong.
- [ ] The meaning of `None`/`0`/empty is stated where it is not obvious.
- [ ] Facts documented elsewhere are linked, not copied.
- [ ] Names, literals and paths are in backticks.
- [ ] `./scripts/check-rust.sh` passes, so every intra-doc link resolves.
