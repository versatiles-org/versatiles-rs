# Writing VPL operation and parameter descriptions

Every VPL operation documents itself through the doc comments on its `Args`
struct. The `VPLDecode` derive turns them into the operation reference
(`versatiles help pipeline`, `versatiles_pipeline/README.md`), the metadata an
editor uses for hovers and completion, and the TypeScript bindings. One doc
comment, several audiences — so the shape matters as much as the wording.

This page is the contract. `docs_style.rs` enforces the mechanical half of it as
a test; the rest is judgement.

## Where the text ends up

```rust
#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a CSV file with longitude/latitude columns and emits MVT point tiles.
///                                    ^-- summary: one sentence
///
/// Rows are read once at build time to derive the tile pyramid.
/// ^-- details: paragraphs, tables and fenced examples all render
struct Args {
    /// Filename of the CSV file, relative to the VPL file.
    /// ^-- one bullet in the generated "### Parameters" list
    filename: String,
}
```

| Text                                 | Reached through           | Rendered as                                        |
| ------------------------------------ | ------------------------- | -------------------------------------------------- |
| First paragraph of the struct doc    | `doc_summary()`           | Tooltip, node picker entry, the line under `## op` |
| Everything after the first paragraph | `doc_details()`           | Markdown, paragraph structure preserved            |
| A field's doc comment                | `field_metadata()[i].doc` | One bullet in the generated `### Parameters` list  |

**Wrapping in the source is cosmetic.** An operation's doc comment is read the
way Markdown reads it: the line breaks inside a paragraph are not meaningful, so
each paragraph is folded back onto one line before it is rendered or handed to
an editor. Wrap the comment to whatever width suits the file. Fenced blocks,
tables, headings and list items keep their own lines, because there the breaks
_are_ meaningful.

**The one hard constraint:** a field's doc comment lines are joined with a
single space. Blank lines, `-` lists and fenced blocks inside a field doc do not
become blank lines, lists or code — they collapse into one run-on line with
stray double spaces in it. Anything that needs structure belongs in the
operation's details, which keep it.

## Operation summary

The first paragraph, and the only text some readers ever see.

1. **One sentence**, present tense, third person, opening with a verb:
   `Reads …`, `Generates …`, `Filters …`, `Converts …`. Not the imperative
   (`Generate …`), not a bare noun phrase, and never a description of the Rust
   struct (`Arguments for the … operation.`).
2. **Say what comes out**, not how it is implemented. Implementation notes are
   details, or code comments.
3. **Do not restate the operation's name.** `raster_overscale` does not need to
   begin "Raster overscale operation — "; the heading already says that.
4. Ends with a period. It carries no link and no parameter list.

```rust
/// Overlays multiple raster tile sources on top of each other.   // good
/// Vector overzoom operation - generates vector tiles beyond …   // restates the name
/// Arguments for the `vector_update_properties` operation.       // describes the struct
/// Flattens (translucent) raster tiles onto a background         // no period
```

## Operation details

Optional, and the right home for everything the summary and the parameter
bullets cannot carry: how the parameters interact, why a default is what it is,
worked examples, a table of useful combinations.

- Write full Markdown. Paragraphs, tables and ` ```vpl ` blocks all render.
- **Never hand-write a parameter list.** The reference generates `### Parameters`
  from the fields; a second, prose copy drifts within one release. Delete
  `### Arguments` sections.
- External links live here rather than in a parameter, and only when they carry
  something the text cannot state itself — a spec, a lookup table. Prefer
  pointing at in-tool help (`versatiles help source`) over a URL.

## Parameter description

One flowing run of sentences. Order: **what it is → units and range → how it
interacts → default.**

1. **No structure.** No blank lines, no `-` lists, no fenced code, no headings.
   See the hard constraint above.
2. Starts with a capital, ends with a period.
3. **Open with a noun phrase naming the value**, without a leading article:
   `Filename of the CSV file`, `Highest zoom level emitted`,
   `Edge blur distance in meters`. For a boolean, open with `Whether to …`.
4. **Never repeat what the generator already prints.** It renders the parameter
   name, its type, `(required)` / `(optional)`, and — for enum-typed parameters
   — the accepted values, taken from the type's own `variants()`. A doc comment
   that lists them again is a second copy that will drift.
5. **Units and range go right after the noun phrase**, not at the end:
   `in pixels`, `in meters`, `between 0 and 100`.
6. **Defaults come last, always in the same form** — one sentence beginning
   "Defaults to". Never `Default:`, never a trailing parenthesis, never "If not
   specified". A computed default is spelled out rather than left implicit. A
   required parameter gets no default sentence at all. A default that is a
   literal value is also **declared on the field**; see "Declaring a default"
   below.
7. **Cross-references** name the other parameter in backticks and say what the
   relation is: "Mutually exclusive with", "Requires", "Overrides".
8. **Literals, paths, identifiers and values go in backticks**, not in straight
   quotes. Introduce an example with "for example", never with "e.g." or a
   "For example:" lead-in.
9. **No links.** If a reader needs one, the operation's details is where it
   goes. A URL used as an example _value_ is fine, since it sits in backticks.

```rust
/// Highest zoom level to emit. Defaults to `14`.                  // good
/// Highest zoom level emitted (default 14).                       // wrong form, wrong place
/// The maximum zoom level. If not specified, 14 is used.          // wrong form
/// Zoom level. Values: mvt, png, webp. Defaults to `mvt`.         // the type prints these
/// Tile size in pixels. Must be 256 or 512. (optional)            // the bullet prints this
```

### Length: under 100 characters

A parameter description is a label, not an explanation. **Keep it under 100
characters**, which is one line in most terminals and enough for what the value
is plus what it defaults to. `docs_style.rs` fails the build above that.

The cap is not a squeeze on useful content — it is what forces the useful
content into the right place. Everything a reader needs _at the point of use_
fits: what the value is, its unit, its default. Everything else — why the
default is what it is, what goes wrong without it, how three parameters
interact — belongs in the operation's details, where paragraphs actually render
and the reader has the whole operation in view. Text buried mid-bullet in a
parameter list is the worst place for it either way.

So when a description will not fit, the fix is almost never to compress the
wording. It is to notice which half of the sentence was rationale and move it.

### Say nothing the reader already knows

Cut anything true of every parameter, obvious from the type, or describing an
error the reader will see anyway:

- **Repository-wide conventions.** Every path parameter resolves relative to the
  `.vpl` file. That is stated once in `help.md`; no parameter repeats it.
- **The obvious failure.** "…and errors out if the schema does not name one",
  "…setting it is an error" — a reader who hits it gets the message. Say it only
  where the behaviour would surprise someone who did _not_ hit it.
- **Restating the name.** `layer_name` does not need "Name of the layer…" spelled
  out twice over.
- **Filler.** "used to", "which is what lets you", "in order to", "Optional".

## Checklist

Before adding or editing an operation:

- [ ] Summary is one sentence, verb-first, present tense, ends with a period.
- [ ] Summary does not restate the operation name or describe the Rust struct.
- [ ] Details carry no hand-written parameter list.
- [ ] Every parameter has a description.
- [ ] No parameter description contains a blank line, a list marker or a URL.
- [ ] No parameter description repeats the type, `(required)`/`(optional)`, or
      an enum's accepted values.
- [ ] Every default reads `Defaults to X.` and comes last.
- [ ] Every literal default is declared with `#[vpl(default = "…")]`.
- [ ] Every parameter description is under 100 characters.
- [ ] Nothing repeats a convention already stated in `help.md`.
- [ ] Ran `./scripts/build-docs-readme.sh` so the generated reference matches.

## Declaring a default

The sentence tells a *reader* what happens when a parameter is left out. A
generated form needs the same fact as data, or it shows an empty box for
`from_color`'s `color` — which will use `000000` — and an identical empty box
for `from_csv`'s `lon_column`, whose absence is an error.

So a literal default is written twice, once for each audience, and
`docs_style.rs` holds the two against each other in both directions:

```rust
/// Hex colour, `RRGGBB` or `RRGGBBAA`. Defaults to `000000`.
#[vpl(default = "000000")]
color: Option<String>,
```

The attribute is **VPL text**, not a Rust expression: it is what a form shows
and what a caller writes into the document to make the default explicit, so
`"000000"`, `"false"` and `"[0,0]"` are all plain strings. It reaches consumers
as `VPLFieldMeta::default`.

Only a *literal* gets one. These do not, and the test does not ask for them:

- **A computed default.** "Defaults to the source's highest." — there is no
  value to write. `VPLFieldMeta::default` is `None`, and a form should say
  nothing rather than invent something.
- **A reference to another parameter.** `level_min` defaults to `level_max`;
  the backticks name a parameter, not a value.
- **No default at all**, which is not the same as required: `filter`'s `bbox`
  clips nothing when unset, and that absence *does* something.

## Accepted values come from the type

A parameter whose value is one of a fixed set gets a type that says so, and the
reference renders the list from that type's `variants()`:

```markdown
- _`format`: RasterTileFormat (optional)_ - Values: `avif`, `jpg`, `png`, `webp`. …
```

So a parameter typed `TileFormat` advertises _every_ format the core knows,
including ones the operation cannot produce. When an operation accepts a subset,
give it a subset type — see `helpers::tile_format_subset` — rather than
narrowing it in prose. That keeps the reference honest and moves the rejection
into `check`, which needs no I/O.

Adding a new one: implement `TryFrom<&str>` and `variants()`, add a
`TypeMapping` entry in `versatiles_derive/src/decode_vpl.rs`, and add the
round-trip test that keeps `variants()` and `TryFrom` in step.
