---
id: scenario-view-derivation
kind: doc
title: Scenario to dynamic view derivation feasibility
---

# Scenario to dynamic view derivation feasibility

Date: 2026-09-14

Ticket: [#218](https://github.com/TudorAndrei/criv/issues/218).
Map: [#198](https://github.com/TudorAndrei/criv/issues/198).
Decision that consumes this: [#207](https://github.com/TudorAndrei/criv/issues/207).

## Question

Two halves. What does the LikeC4 dynamic view DSL actually accept, measured
against the installed LikeC4 1.59.2 rather than recalled. And does the
matching rule from [#217](https://github.com/TudorAndrei/criv/issues/217)
produce a usable flow from ordinary Gherkin prose, measured against this
repository's own model in `docs/architecture/`.

## Answer

The DSL half is permissive in the directions the design needs and strict in
two directions that break it. A dynamic view step may join any two existing
elements, including a pair the model does not connect and an element with
itself, and every step is a plain arrow with a string title. But a step
between an ancestor and a descendant is an error, an empty dynamic view is
silently dropped from the layouted model, and any error in derived text
blanks the architecture State for every view, because the bridge lays out
nothing when the diagnostics list is not empty.

The matching half fails on naive prose and passes on model-aware prose. From
14 naive steps, 8 matched, 6 were gaps, and every one of the 8 matches was
ambiguous or wrong. The derived naive flow is three self-relations on `criv`
and one arrow in the wrong direction. From 21 model-aware steps, 21 matched
with 0 gaps, and 9 of the 18 derived arrows follow a declared relation. The
difference is not the rule. It is whether the writer knew the element titles.

Labels are fine when the participant leads the sentence. Text after the
matched title, with its first letter capitalised, met the ADR-0077 rule in 18
of 18 model-aware steps and in 5 of 8 naive matches.

---

## Method

- Workspace: a copy of `docs/architecture/` at commit `34d394f`, with probe
  files under `probes/`. 125 elements, 248 distinct declared source-target
  pairs.
- Compiler: the installed `likec4` 1.59.2 from the root `package-lock.json`.
  `likec4 validate --json --no-layout` for acceptance, `likec4 export json`
  for the layouted shape, and the criv bridge `assets/likec4-bridge.mjs`
  itself, run by hand, for what State would receive.
- Matcher: a script that applies #217 by machine. It finds an element
  identifier (last FQN segment) or an element title in each step sentence as
  a whole word, case-insensitive and case-sensitive. Counts below are its
  output, not a reading.
- Order: the naive scenarios were written and saved before the model files
  were read. The hand-authored `views/dynamic/refresh-session.c4` had been
  read, because the ticket asked for it. Nothing in the naive text quotes it.

## What the dynamic view DSL permits

Each row is one probe file. "Accepted" means zero errors from `validate`.
The error text is verbatim.

| Question from the ticket | Probe | Result |
| --- | --- | --- |
| Must every participant exist in the model? | `ghostElement -> criv 'x'` | Rejected: `Could not resolve reference to Referenceable named 'ghostElement'.` and `Source not found (not parsed/indexed yet)`. Same for an unknown target. |
| Can a step carry no target? | `codingAgent` alone | Rejected: `Expecting: one of these possible Token sequences: 1. [<-] 2. [Dot] 3. [->] 4. [-[]`. |
| Can a participant appear with no arrow? | `include codingAgent` | Accepted. The element is a node of the view with no edge. |
| Is a self-relation legal? | `criv -> criv 'Talks to itself'` | Accepted. Exported as one node and one edge with `source == target`. |
| Can a step be a note? | `note 'x'` as a statement | Rejected: `Could not resolve reference to Referenceable named 'note'.` |
| Can a step be a group? | `group 'Then' { ... }` | Rejected: `Could not resolve reference to Referenceable named 'group'.` and `Expecting end of file but found '}'`. |
| Can a step carry notes? | `a -> b { notes '''...''' }` | Accepted, with or without a title. Exported as `notes: { md }`. |
| Must a step match a declared relation? | `githubActions -> obsidian 'x'` | Accepted. Exported edge has `relations: []`. |
| May a step reverse a declared relation? | `criv.stateStore -> criv.cli.statePublisher 'x'` | Accepted. `relations: []`. |
| May a step join an ancestor and a descendant? | `criv -> criv.cli 'x'` | Rejected: `Invalid parent-child relationship`. Same for grandchild to grandparent. |
| Does `<-` exist? | `codingAgent <- criv.cli 'Returns the report'` | Accepted. Exported with `source: criv.cli`, `target: codingAgent`, `dir: "back"`. |
| Do parallel and nested blocks exist? | `parallel { ... opt { ... } }`, `alt { when ... else ... }`, chained `a -> b -> c` | Accepted. |
| Can `parallel` nest in `parallel`? | `parallel { parallel { } }` | Rejected: `Nested parallel blocks are not allowed`. |
| Is an empty dynamic view legal? | `dynamic view p08 { title 'x' }` | Accepted by `validate`. Dropped at layout, see below. |
| Is a view with only `include` legal? | `dynamic view p14 { include codingAgent }` | Accepted by `validate`. Dropped at layout, see below. |
| Kebab view identifier | `dynamic view a-refresh-publishes-state` | Accepted. |
| Leading digit | `dynamic view 1st-scenario` | Rejected: `unexpected character: ->1<- at offset: 23`. |
| Colon | `dynamic view scenario:place-an-order` | Rejected: `Expecting token of type '}' but found ':'`. |
| Same identifier as a hand-authored view | `dynamic view refreshSession` | Rejected in **both** files: `Duplicate view 'refreshSession'` at the probe and at `views/dynamic/refresh-session.c4:3`. |
| Step with no title | `codingAgent -> criv` | Accepted. |
| Step title with `\'`, `"`, `<method>`, `\|`, `#`, a path, or empty | five variants | All accepted. |

### What a title, a description, and notes accept

Measured from the exported JSON of one probe view.

- View `title`: a plain string. Exported as a string.
- View `description`: a string or a `'''` Markdown block. Exported as
  `{ md: "..." }`.
- Step title: the string after the arrow. Exported as `label`. LikeC4 wraps
  a long label with `\n` at layout, for example
  `reports a broken source link with the \nfile path`.
- Step body: `title`, `description`, `technology`, `notes`, `metadata`,
  `navigateTo`. A body `title` replaces the arrow string as `label`.
  `description` in single quotes is exported as `{ txt }`; `notes` as
  `{ md }`; `technology` as a string.
- A step with no title, or an empty `''` title, over a pair with exactly one
  declared relation takes that relation's label. `codingAgent -> criv` shows
  `Checks changes and queries repository knowledge`, the label of the
  declared `codingAgent -> criv.cli`. Over a pair with no declared relation
  the label is `null`. Over a pair with two candidate relations, here
  `criv.cli -> criv.stateStore` at container and component level, the label
  is also `null`.

### Three facts that break parts of the design

1. **An empty derived view vanishes without a diagnostic.** The two views
   with no step passed `validate`, then failed layout with
   `actors array must not be empty` in `calcSequenceLayout`. `likec4 export`
   logs `Fail layout view p08` and omits the view. The criv bridge runs with
   `logger: false`, so its response was `valid: true`, `diagnostics: []`,
   zero bytes on standard error, and 44 views instead of 46. A scenario whose
   steps are all gaps, or whose only match is the `Given`, produces a view
   that State never shows and nothing reports. This is the exact silent
   shrink that [[0079-no-fallback-view-in-the-c4-preview|ADR-0079]] forbids.
   The derivation must refuse to emit a view with zero steps and report the
   gap itself.
2. **One error in derived text blanks every view.** The bridge computes
   `layoutedModel` only when `getErrors()` is empty. A derived view with an
   unresolved reference, an ancestor-descendant step, or an identifier that
   collides with an agent-authored view makes `model: null` for the whole
   workspace, and the collision case also marks the agent's own file as
   broken. So the derivation cannot lean on LikeC4 to reject a bad step. It
   must guarantee its own text is valid before the bridge sees it: every
   participant resolved, no step between an element and its ancestor, and an
   identifier that no `.c4` file declares.
3. **The scenario identity from #200 is not a view identifier.**
   `features/checkout.feature#scenario:place-an-order` contains `/`, `.`,
   `#`, and `:`, and `:` alone is rejected. A kebab name such as
   `a-refresh-publishes-state` is accepted. The derived view needs a mapped
   identifier, and the mapping must avoid the agent's view names.

### One fact that touches #213

The public `LikeC4` namespace in 1.59.2 has two constructors:
`fromSource(likec4SourceCode: string)` and `fromWorkspace(path?: string)`.
The first takes one document with no workspace, the second a directory with
no extra document. Building the view "in memory" next to the on-disk
workspace, as [#213](https://github.com/TudorAndrei/criv/issues/213)
decided, has no entry point at this level. It needs the language-services
layer under `LikeC4`, which was not probed. Recorded as unconfirmed below.

## Whether the matching rule works on real data

### Naive scenarios

Written before the model files were read, as a product person would write
them.

```gherkin
Feature: criv keeps the vault and the code in step

  Scenario: A refresh publishes State
    Given a developer changes a source file in the repository
    When the watch task runs a refresh
    Then criv validates the vault
    And criv writes the new State to .criv/state.json
    And the Obsidian plugin shows the new State

  Scenario: An ADR reconciles on push
    Given an ADR governs a source directory
    And a developer pushes a commit that changes a governed file
    When the pre-push hook runs criv enforce
    Then criv reports the governed files that changed
    And the push is blocked until the ADR is reconciled

  Scenario: A check reports a broken source link
    Given a doc links to a symbol that no longer exists
    When the user runs criv check
    Then criv reports a broken source link with the file path
    And the check exits with a non-zero code
```

Matcher output, identical case-insensitive and case-sensitive:

| Step | Result |
| --- | --- |
| a developer changes a source file in the repository | `repository` (softwareSystem 'Project repository'), by identifier only |
| the watch task runs a refresh | gap |
| criv validates the vault | `criv` (softwareSystem) **and** `criv.cli.commandInterface.rustCriv` (module titled 'criv') |
| criv writes the new State to .criv/state.json | same two elements |
| the Obsidian plugin shows the new State | `obsidian` (external softwareSystem 'Obsidian'), not `criv.obsidianPlugin` ('Obsidian companion plugin') |
| an ADR governs a source directory | gap |
| a developer pushes a commit that changes a governed file | gap |
| the pre-push hook runs criv enforce | same two `criv` elements |
| criv reports the governed files that changed | same two `criv` elements |
| the push is blocked until the ADR is reconciled | gap |
| a doc links to a symbol that no longer exists | gap |
| the user runs criv check | same two `criv` elements |
| criv reports a broken source link with the file path | same two `criv` elements |
| the check exits with a non-zero code | gap |

**14 steps, 8 matched, 6 gaps, 6 of the 8 matches ambiguous, 1 of the 8
wrong, 1 correct by luck of the identifier.** Nothing in the naive text hit
a title of a component. The words a person reaches for, `watch`, `refresh`,
`ADR`, `push`, `hook`, `check`, `doc`, `symbol`, `State`, are all in the
model as parts of titles (`Refresh coordinator`, `Governance engine`,
`Published state store`) and never as whole titles.

The derived view, with the ambiguous `criv` resolved to the system by hand:

```likec4
dynamic view derivedNaiveRefresh {
  title 'Derived naive / A refresh publishes State'
  include repository
  repository -> criv 'validates the vault'
  criv -> criv 'writes the new State to .criv/state.json'
  criv -> obsidian 'plugin shows the new State'
}
```

It compiles and lays out. It is wrong three times: the first arrow points
from the repository to criv, where the model says criv reads the repository;
the second is a self-relation; the third targets the external Obsidian
system rather than the plugin. Scenarios 2 and 3 each reduce to one
self-relation `criv -> criv`. Across the three naive views: 5 edges, 1 with a
declared relation (`criv -> obsidian`, through the nested plugin relation),
4 without.

### Model-aware scenarios

Written after reading the element titles, one title per step.

```gherkin
Feature: criv keeps the vault and the code in step

  Scenario: A refresh publishes State
    Given the Coding agent changes vault content
    When the Refresh coordinator starts a refresh
    And the Source intelligence builds one Source state
    And the Vault model loads the vault
    And the C4 service compiles the LikeC4 workspace
    And the Governance engine checks publication blockers
    Then the State publisher commits State
    And the Published state store holds the new State
    And the Obsidian companion plugin shows the new State

  Scenario: An ADR reconciles on push
    Given the Coding agent pushes a commit that changes a governed file
    When the Governance engine runs the pre-push stage
    And the Repository history lists the commits behind the push
    And the Source intelligence resolves the governed symbols
    Then the Governance engine reconciles the ADR
    And the Repository files record the reconciled ADR

  Scenario: A check reports a broken source link
    Given the Repository maintainer runs a check
    When the Vault model loads the vault
    And the Source intelligence resolves every source target
    Then the Governance engine reports the broken source link
    And the Diagnostic locations give the file path and line
    And the Command interface exits with a non-zero code
```

**21 steps, 21 matched, 0 gaps.** Steps with more than one candidate: 4
case-insensitive, 1 case-sensitive.

- `Obsidian companion plugin` also matches the title `Obsidian` of the
  external system, in both modes. A title that contains another title
  collides.
- `Repository history`, `Repository files`, and `Repository maintainer`
  also match the identifier `repository` case-insensitively. A common word
  used as an identifier collides with every title that contains it.
- Longest match wins would resolve all four. Case-sensitive matching alone
  resolves three.

The derived view for scenario 1, with the #217 rule applied as written: the
matched element is the participant, the previous step's element is the
source.

```likec4
dynamic view derivedRefreshPublishesState {
  title 'Derived / A refresh publishes State'
  include codingAgent
  codingAgent -> criv.cli.refreshCoordinator 'Starts a refresh'
  criv.cli.refreshCoordinator -> criv.cli.sourceIntelligence 'Builds one Source state'
  criv.cli.sourceIntelligence -> criv.cli.vaultModel 'Loads the vault'
  criv.cli.vaultModel -> criv.cli.architectureService 'Compiles the LikeC4 workspace'
  criv.cli.architectureService -> criv.cli.governanceEngine 'Checks publication blockers'
  criv.cli.governanceEngine -> criv.cli.statePublisher 'Commits State'
  criv.cli.statePublisher -> criv.stateStore 'Holds the new State'
  criv.stateStore -> criv.obsidianPlugin 'Shows the new State'
}
```

All three model-aware views compile, lay out, and appear in the bridge
response. Declared-relation check from the exported edges:

| Derived view | Edges | With a declared relation | Without |
| --- | ---: | ---: | ---: |
| A refresh publishes State | 8 | 5 | 3 |
| An ADR reconciles on push | 5 | 2 | 3 |
| A check reports a broken source link | 5 | 2 | 3 |
| **Total** | **18** | **9** | **9** |

The 9 undeclared arrows are not matching failures. They come from the
"previous step is the source" rule. It can only draw a chain. The
hand-authored `refreshSession` view has the Refresh coordinator as the
source of 6 of its 11 steps, a hub. The chain rule turns
`refreshCoordinator -> vaultModel` into `sourceIntelligence -> vaultModel`,
and `obsidianPlugin -> stateStore 'Reads the published state'` into
`stateStore -> obsidianPlugin`. A scenario cannot express a hub under the
rule unless one step may name two elements, first as source and second as
target. Each of the 11 hand-authored steps can be written that way in one
sentence. That is a #207 question, not a finding against #217.

### Naive against model-aware

| Measure | Naive | Model-aware |
| --- | ---: | ---: |
| Steps | 14 | 21 |
| Matched | 8 | 21 |
| Gaps | 6 | 0 |
| Matches with more than one candidate | 6 | 4 (1 case-sensitive) |
| Matches on the wrong element | 1 | 0 |
| Derived edges | 5 | 18 |
| Edges with a declared relation | 1 | 9 |
| Self-relations | 3 | 0 |
| Views that would be dropped at layout | 0 | 0 |

## What the labels look like

Label rule tested: the step text after the matched title, first letter
capitalised. ADR-0077 asks for a capital letter, a present-tense verb, and
no trailing preposition.

Model-aware, 18 of 18 pass:

| Step text | Label | ADR-0077 |
| --- | --- | --- |
| the Refresh coordinator starts a refresh | Starts a refresh | pass |
| the Source intelligence builds one Source state | Builds one Source state | pass |
| the Vault model loads the vault | Loads the vault | pass |
| the C4 service compiles the LikeC4 workspace | Compiles the LikeC4 workspace | pass |
| the Governance engine checks publication blockers | Checks publication blockers | pass |
| the State publisher commits State | Commits State | pass |
| the Published state store holds the new State | Holds the new State | pass |
| the Obsidian companion plugin shows the new State | Shows the new State | pass |
| the Governance engine runs the pre-push stage | Runs the pre-push stage | pass |
| the Repository history lists the commits behind the push | Lists the commits behind the push | pass |
| the Source intelligence resolves the governed symbols | Resolves the governed symbols | pass |
| the Governance engine reconciles the ADR | Reconciles the ADR | pass |
| the Repository files record the reconciled ADR | Record the reconciled ADR | pass, plural verb |
| the Vault model loads the vault | Loads the vault | pass |
| the Source intelligence resolves every source target | Resolves every source target | pass |
| the Governance engine reports the broken source link | Reports the broken source link | pass |
| the Diagnostic locations give the file path and line | Give the file path and line | pass, plural verb |
| the Command interface exits with a non-zero code | Exits with a non-zero code | pass |

Naive, 5 of 8 pass mechanically, 2 of those 5 are meaningless:

| Step text | Label | ADR-0077 |
| --- | --- | --- |
| a developer changes a source file in the repository | (empty; the match ends the sentence) | fail, no label |
| criv validates the vault | Validates the vault | pass |
| criv writes the new State to .criv/state.json | Writes the new State to .criv/state.json | pass, ends with a path |
| the Obsidian plugin shows the new State | Plugin shows the new State | fail, starts with a noun |
| the pre-push hook runs criv enforce | Enforce | pass in form, meaningless |
| criv reports the governed files that changed | Reports the governed files that changed | pass |
| the user runs criv check | Check | pass in form, meaningless |
| criv reports a broken source link with the file path | Reports a broken source link with the file path | pass |

Two observations for #207:

- The whole step sentence as the label fails the verb rule every time the
  sentence starts with `the`, `a`, or the subject, which is every step in
  both sets. The label has to be a slice.
- An empty title over a pair with exactly one declared relation shows that
  relation's agent-authored label, which already meets ADR-0077. Over an
  undeclared pair it shows nothing. So "no title, step text in `notes`" is
  legal DSL and gives a clean label for 9 of the 18 model-aware edges and a
  blank arrow for the other 9.

## Facts for #207 to absorb

These are measured facts, not decisions.

1. Every participant must exist. LikeC4 rejects an unknown reference. The
   #217 premise holds.
2. A `Given` with a participant and no arrow is `include <element>`. It
   works only when at least one step follows; alone it is dropped silently.
3. A `Then` with no participant has two legal homes: `notes` on the previous
   step, or nothing. `note` and `group` are not statements in a dynamic view.
4. A self-relation is legal and renders. The naive set produced three.
5. A step between an element and its ancestor is an error, and the error
   blanks every view in State. Naive prose that names `criv` in one step and
   `criv CLI` in the next produces exactly this pair.
6. An undeclared pair is legal, renders, and is marked `relations: []` in
   the layouted edge. criv can report "the architecture has no such path"
   from the bridge output without its own graph work.
7. A derived view with zero steps must not reach the bridge.
8. The derived view identifier must be a LikeC4 identifier, must not collide
   with any agent-authored view, and cannot be the #200 scenario identity as
   is.
9. Matching on the identifier as well as the title creates collisions with
   common words (`repository`, `criv`). Matching on the title alone, longest
   match first, resolved every collision in both sets.
10. The "previous step is the source" rule draws chains only. Half of the
    model-aware arrows contradict the declared model for this reason alone.

## Unconfirmed

- Whether the language-services layer under `LikeC4` accepts a virtual
  document beside an on-disk workspace. Only the two public constructors
  were read.
- Whether the silent drop at layout is the same for an element view with no
  node. Only dynamic views were probed.
- Whether a `variant sequence` view with a self-relation renders a lifeline
  self-message. The self-relation probe used the default variant.
- Whether long label wrapping with `\n` reaches the editor renderer or is a
  CLI export artefact.

## Reproduction

All probe files, the two feature files, the matcher, and the bridge runner
were kept in the session scratchpad and are reproducible from this note:
copy `docs/architecture/` to a scratch directory, add each snippet from the
table above as `probes/<name>.c4`, and run

```sh
npx likec4 validate --json --no-layout <scratch-dir>
npx likec4 export json -o model.json --pretty <scratch-dir>
```

For the bridge result, run `assets/likec4-bridge.mjs` with the protocol
placeholder replaced by `1`, from the repository root so that
`require.resolve('likec4')` finds the pinned package, with the scratch
directory as the first argument and `0` as the second.
