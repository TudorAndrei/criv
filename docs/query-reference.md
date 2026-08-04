---
id: query-reference
kind: doc
title: criv query reference
targets:
  symbols:
    - src/query.rs
---

# criv query reference

`criv query` asks the loaded vault graph focused questions and prints one row
per result. Add `--format json` to any query for a JSON array of rows:

```sh
criv query nodes --kind decision --format json
```

<!-- Generated from `criv --usage`; do not hand-maintain these command sections. -->

## `criv query`

- **Usage**: `criv query <SUBCOMMAND>`

## `criv query next-adr-id`

- **Usage**: `criv query next-adr-id [--format <FORMAT>]`

Print the next ADR id after the highest existing ADR id

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query callers`

- **Usage**: `criv query callers [--format <FORMAT>] <SYMBOL>`

List source symbols that call the requested symbol

### Arguments

#### `<SYMBOL>`

Source path or symbol selector

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query callees`

- **Usage**: `criv query callees [--format <FORMAT>] <SYMBOL>`

List source symbols called by the requested symbol

### Arguments

#### `<SYMBOL>`

Source path or symbol selector

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query attack-surface`

- **Usage**: `criv query attack-surface [--format <FORMAT>]`

List exported or public source symbols in the source graph

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query targets`

- **Usage**: `criv query targets [--format <FORMAT>] <NOTE_ID>`

List source and pattern targets declared or linked by a note

### Arguments

#### `<NOTE_ID>`

Note id or unique note name

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query cites`

- **Usage**: `criv query cites [--format <FORMAT>] <NOTE_ID>`

List notes, sources, and patterns cited by a note

### Arguments

#### `<NOTE_ID>`

Note id or unique note name

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query cited-by`

- **Usage**: `criv query cited-by [--format <FORMAT>] <NOTE_ID>`

List notes that cite the requested note

### Arguments

#### `<NOTE_ID>`

Note id or unique note name

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query orphan-docs`

- **Usage**: `criv query orphan-docs [--format <FORMAT>]`

List documentation notes without incoming or outgoing note citations

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query references`

- **Usage**: `criv query references [--format <FORMAT>] <SYMBOL>`

List notes that reference a source path or symbol

### Arguments

#### `<SYMBOL>`

Source path or symbol selector

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query governs`

- **Usage**: `criv query governs [--format <FORMAT>] <ADR_ID>`

List source files governed by a decision

### Arguments

#### `<ADR_ID>`

ADR id

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query governing`

- **Usage**: `criv query governing [--format <FORMAT>] <SYMBOL>`

List decisions that govern a source path or symbol

### Arguments

#### `<SYMBOL>`

Source path or symbol selector

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query coverage`

- **Usage**: `criv query coverage [--by <BY>] [--format <FORMAT>]`

Summarize source governance coverage

### Flags

#### `--by <BY>`

Group coverage rows by module or ADR

**Choices:**

- `module`
- `adr`

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query nodes`

- **Usage**: `criv query nodes [FLAGS]`

List source, note, or decision nodes

### Flags

#### `--kind <KIND>`

Restrict nodes to code, documentation, or decisions

**Choices:**

- `code`
- `doc`
- `decision`

#### `--without-docs`

Restrict code nodes to symbols that no note references

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query c4-code`

- **Usage**: `criv query c4-code [--format <FORMAT>] <PATH_GLOB>`

Emit focused LikeC4 source for modules in a source path glob

### Arguments

#### `<PATH_GLOB>`

Source path or component/module glob

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

## `criv query diff`

- **Usage**: `criv query diff [--format <FORMAT>] <REF_A> <REF_B>`

Compare two state snapshots or git refs

### Arguments

#### `<REF_A>`

Left snapshot hash, `latest`, or git ref

#### `<REF_B>`

Right snapshot hash, `latest`, or git ref

### Flags

#### `--format <FORMAT>`

Select text rows or a JSON array of rows

**Choices:**

- `text`
- `json`

**Default:** `text`

`diff` resolves `latest` through `.criv/latest`, hex-like values through
`.criv/snapshots/<hash>.json`, and any other value through
an embedded lookup of `.criv/state.json` in the requested repository ref. It
does not invoke the `git` executable.
