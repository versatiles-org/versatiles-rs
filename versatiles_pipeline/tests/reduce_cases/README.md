# Reduction cases

Each file here is a JSON array of cases for `vector_reduce_to_style`. One case
is a small style, one tile of hand-made features, and two expectations: what
MapLibre **draws** and what the reduction **keeps**. The runner is
`every_case_holds` in
`src/operations/vector/vector_reduce_to_style_cases.rs`:

```sh
cargo test -p versatiles_pipeline --lib every_case_holds -- --nocapture
```

`--nocapture` prints a summary: how many cases are exact, how many are refused,
and how many keep more than is drawn, by reason.

## A case

```json
{
  "name": "a match filter keeps every feature in its zoom range",
  "style": [
    {
      "source-layer": "water",
      "filter": ["match", ["get", "kind"], ["lake"], true, false]
    }
  ],
  "zoom": 14,
  "tile": {
    "water": [
      { "id": 1, "kind": "lake" },
      { "id": 2, "kind": "river" }
    ]
  },
  "drawn": { "water": { "1": ["kind"] } },
  "kept": { "water": { "1": ["kind"], "2": ["kind"] } },
  "widening": {
    "kind": "unreadable-filter",
    "why": "`match` is not in the predicate set, so the filter keeps everything."
  }
}
```

| Key               | Meaning                                                                                                                                                         |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `name`            | Unique across all files.                                                                                                                                        |
| `style`           | An array of style layers, or a whole style object when `sources` matter. A layer without `id` gets one.                                                         |
| `source`          | Optional: the operation's `source` argument, a list of IDs.                                                                                                     |
| `tileset_maxzoom` | Optional: the tileset's deepest zoom. Defaults to 14.                                                                                                           |
| `tileset_layers`  | Optional: the layer names the TileJSON lists. Defaults to the layers of `tile`.                                                                                 |
| `zoom`            | The tile zoom, or a list of zooms.                                                                                                                              |
| `tile`            | Layer → features. A feature has an `id` (unique in its layer), an optional `$geometry` (`Point`, the default, `LineString` or `Polygon`), and its properties.   |
| `drawn`           | What MapLibre draws: layer → feature ID → the properties MapLibre reads from that feature. With a list of zooms, keyed by zoom first: `{"13": {…}, "14": {…}}`. |
| `kept`            | What the reduction keeps, in the same shape.                                                                                                                    |
| `widening`        | Required exactly when `kept` is larger than `drawn`: a `kind` from the list below, and `why` in one sentence.                                                   |
| `error`           | Instead of `zoom`, `tile`, `drawn` and `kept`: a substring of the error the build must fail with.                                                               |
| `fields`          | Optional: the TileJSON `vector_layers` fields after the reduction, layer → names.                                                                               |

**The properties MapLibre reads from a feature** are those the `filter` of any
style layer of its source-layer reads at that zoom — a filter runs on every
feature, and stripping what it reads can change its answer — and those the
`paint` and `layout` of the style layers drawing it read.

Property values are strings, booleans and numbers; a whole number becomes an
`Int`, any other a `Double`. A typed number is `{"uint": 2}`, `{"int": -2}`,
`{"float": 2.5}` or `{"double": 2.5}`. Unknown keys are an error, so a typo
cannot skip a check.

## What the runner checks

1. **The contract:** the output holds everything in `drawn`. A failure here is
   a bug in the reduction.
2. **The behaviour:** the output is exactly `kept`. Keeping less, while still
   holding everything drawn, is reported as an improvement: update `kept`.
3. **The case:** `kept` holds everything in `drawn`, and differs from it
   exactly when there is a `widening`.
4. `fields`, when given.

## Widening kinds

| Kind                        | The reduction keeps more because                                                                     |
| --------------------------- | ---------------------------------------------------------------------------------------------------- |
| `unreadable-filter`         | a filter is outside the predicate set: `match`, `case`, `coalesce`, `$type`, substring `in`, `zoom`… |
| `unreadable-negation`       | one unreadable part of `any`, `none` or `!` widens the whole filter.                                 |
| `to-boolean`                | `to-boolean` is read as `has`, which also keeps `""`, `0` and `false`.                               |
| `draws-nothing`             | a feature passes the filter but draws nothing: no icon, opacity 0, empty text.                       |
| `geometry-type`             | a layer type draws only some geometries.                                                             |
| `property-per-source-layer` | properties are kept on every feature of a source-layer, not only where a style layer reads them.     |
| `property-zoom-span`        | one zoom range per property spans a gap between two style layers.                                    |
| `computed-property-name`    | a property name computed at runtime, or `["properties"]`, keeps all of them.                         |
| `generous-field-detection`  | a string is taken for a property name although it is none.                                           |
| `inclusive-maxzoom`         | a layer's `maxzoom` is read as inclusive: one zoom level more.                                       |
| `zoom-clamp`                | something is kept at the deepest zoom although no tile there is requested.                           |

A new kind goes into `WIDENING_KINDS` in the runner and into this table.
