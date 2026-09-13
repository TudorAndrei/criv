---
id: cucumber-dry-run-plan
kind: doc
title: Cucumber dry run as a step-to-definition plan
---

# Cucumber dry run as a step-to-definition plan

Research date: 2026-09-13.

Issue: [#211](https://github.com/TudorAndrei/criv/issues/211).
Map: [#198](https://github.com/TudorAndrei/criv/issues/198).

Earlier research: [#203](https://github.com/TudorAndrei/criv/issues/203) records
the messages contract. This document does not repeat it.

## Question

criv must derive a LikeC4 dynamic view from a Gherkin scenario. The derivation
resolves each step through its step definition to the C4 element that already
holds a `link ... 'source'` for that file. The step-to-definition edge is
therefore a hard dependency.

Does a cucumber dry run give that edge, with no step body executed?

## Answer

**Yes. A dry run gives the step-to-definition edge, and no step body runs.**

This is measured for cucumber-js and cucumber-ruby, and read from source for
cucumber-jvm. A dry run is a real plan phase: matching happens in a separate
phase that runs before execution and does not read the dry-run flag. The
`testCase` envelope therefore carries `stepDefinitionIds` whether or not a body
runs.

Three facts limit the plan:

1. A dry run still loads every step-definition file and executes its top-level
   code. It does not need a running application, but it does need the complete
   module graph to import without error. With a dependency-injection backend
   such as cucumber-spring it also starts the container. So a dry run is a
   cheaper run, not a non-run, and it does not escape
   [[0046-no-native-linting-in-criv-enforce|ADR-0046]].
2. For cucumber-jvm, `stepDefinition.sourceReference` holds a class name and a
   method name with no file. The edge stops at a Java symbol, and criv cannot
   turn that into a file without Java symbol knowledge. **This is the one fact
   that makes the derivation impossible for a language, not merely harder.**
3. Every report path is relative to the working directory of the run, and no
   report records that directory. criv needs it as a separate input.

**One correction to the map.**
[#198](https://github.com/TudorAndrei/criv/issues/198) records that Go, Python,
and Rust "emit nothing". That is true of the messages format only. Four of the
five runners in those languages do give a machine-readable step-to-definition
edge in their own format, and behave gives it from a real dry run. Only
pytest-bdd gives nothing and has no route to anything.

## The measured cucumber-js experiment

The evidence below comes from a scratch project outside the criv repository.
Versions: `@cucumber/cucumber` 13.2.1, `@cucumber/messages` 34.2.0, Node 26.5.1.
The reviewed cucumber-js commit is
[`17a8c39`](https://github.com/cucumber/cucumber-js/tree/17a8c39a8e9dcf87d234c11f7d0fd42d8081b65a),
which is tag `v13.2.1`.

The project holds one feature with three steps, and two step-definition files.
The third step has no definition. Each body and each `Before` hook writes a
marker line to standard error.

### A normal run executes the bodies

```text
$ npx cucumber-js --format message:normal.ndjson features/probe.feature
EXIT=1
SIDE EFFECT: Before hook ran
SIDE EFFECT: cart step body ran
SIDE EFFECT: payment step body ran
```

### A dry run executes nothing

```text
$ npx cucumber-js --dry-run --format message:dry.ndjson features/probe.feature
EXIT=0
(no output on standard error)
```

No marker line appears. The hook did not run, and no step body ran. The exit
code is 0 even though one step is undefined.

### The envelope sets are identical

Counting the envelope key of every NDJSON line of both files gives the same
result:

| Envelope | Normal run | Dry run |
| --- | --- | --- |
| `meta` | 1 | 1 |
| `source` | 1 | 1 |
| `gherkinDocument` | 1 | 1 |
| `pickle` | 1 | 1 |
| `hook` | 1 | 1 |
| `stepDefinition` | 2 | 2 |
| `testRunStarted` | 1 | 1 |
| `testCase` | 1 | 1 |
| `testCaseStarted` | 1 | 1 |
| `testStepStarted` | 4 | 4 |
| `testStepFinished` | 4 | 4 |
| `suggestion` | 1 | 1 |
| `testCaseFinished` | 1 | 1 |
| `testRunFinished` | 1 | 1 |

A dry run omits **no** envelope type. `stepDefinition` envelopes are present.
The only content that disappears is content that only a body can produce: an
`attachment`, a log, and an exception inside a `testStepResult`.

### The dry run carries the complete edge

These are the observed dry-run envelopes, with the identifiers shortened for
reading.

```json
{"stepDefinition":{"id":"509f5a74-...","pattern":{"source":"a cart with one item","type":"CUCUMBER_EXPRESSION"},"sourceReference":{"uri":"features/step_definitions/cart_steps.js","location":{"line":7}}}}
{"stepDefinition":{"id":"77f7b468-...","pattern":{"source":"the customer pays","type":"CUCUMBER_EXPRESSION"},"sourceReference":{"uri":"features/step_definitions/payment_steps.js","location":{"line":3}}}}
```

```json
{"testCase":{"testRunStartedId":"4771c3e0-...","pickleId":"99872f73-...","id":"8c8d5ea2-...","testSteps":[
  {"id":"1afdd931-...","hookId":"d52ae885-..."},
  {"id":"0e3ee08c-...","pickleStepId":"a150ac56-...","stepDefinitionIds":["509f5a74-..."],"stepMatchArgumentsLists":[{"stepMatchArguments":[]}]},
  {"id":"8009c4c3-...","pickleStepId":"4f5b7f7a-...","stepDefinitionIds":["77f7b468-..."],"stepMatchArgumentsLists":[{"stepMatchArguments":[]}]},
  {"id":"598ce06b-...","pickleStepId":"5cc36485-...","stepDefinitionIds":[],"stepMatchArgumentsLists":[]}]}}
```

Every matched step holds exactly one entry in `stepDefinitionIds`, and each
entry matches one `stepDefinition.id`. The undefined step holds an empty list.
The four-link chain that [#203](https://github.com/TudorAndrei/criv/issues/203)
describes is complete.

### Why matching survives a dry run

The edge is built during assembly, and assembly does not know about the flag.
`makeSteps` filters the step definitions for every pickle step with no
condition:

```ts
const stepDefinitions = supportCodeLibrary.stepDefinitions.filter((stepDefinition) =>
  stepDefinition.matchesStepName(pickleStep.text)
)
```

See
[`assemble_test_cases.ts` line 106](https://github.com/cucumber/cucumber-js/blob/17a8c39a8e9dcf87d234c11f7d0fd42d8081b65a/src/assemble/assemble_test_cases.ts#L106).
The complete file holds no reference to `dryRun`.

The flag acts only later, in the runtime. It makes a hook return a `SKIPPED`
result without invoking the body, at
[`executor.ts` line 51](https://github.com/cucumber/cucumber-js/blob/17a8c39a8e9dcf87d234c11f7d0fd42d8081b65a/src/runtime/executor.ts#L51),
and it sets `skip` for the test-case runner at
[`executor.ts` line 127](https://github.com/cucumber/cucumber-js/blob/17a8c39a8e9dcf87d234c11f7d0fd42d8081b65a/src/runtime/executor.ts#L127).

### The step statuses of a dry run

```json
{"testStepFinished":{"testStepId":"1afdd931-...","testStepResult":{"status":"SKIPPED","duration":{"seconds":0,"nanos":0}}}}
{"testStepFinished":{"testStepId":"0e3ee08c-...","testStepResult":{"status":"SKIPPED","duration":{"seconds":0,"nanos":0}}}}
{"testStepFinished":{"testStepId":"8009c4c3-...","testStepResult":{"status":"SKIPPED","duration":{"seconds":0,"nanos":0}}}}
{"testStepFinished":{"testStepId":"598ce06b-...","testStepResult":{"status":"UNDEFINED","duration":{"seconds":0,"nanos":0}}}}
```

A matched step is `SKIPPED`. An undefined step is `UNDEFINED`. Every duration is
zero. The official documentation states the same three effects: no hook runs,
steps report as skipped, and an undefined or an ambiguous step does not fail the
process. See
[`docs/dry_run.md`](https://github.com/cucumber/cucumber-js/blob/17a8c39a8e9dcf87d234c11f7d0fd42d8081b65a/docs/dry_run.md).

`testRunFinished` reports `"success": true`, with one undefined step present:

```json
{"testRunFinished":{"testRunStartedId":"4771c3e0-...","success":true}}
```

The cause is explicit in the source. `shouldCauseFailure` returns `false` first
when `dryRun` is set, before it looks at `AMBIGUOUS`, `FAILED`, or `UNDEFINED`.
See
[`helpers.ts` line 63](https://github.com/cucumber/cucumber-js/blob/17a8c39a8e9dcf87d234c11f7d0fd42d8081b65a/src/runtime/helpers.ts#L63).

**Consequence for criv.** A consumer must not read `testRunFinished.success`
from a dry run. It is `true` for a scenario with no definition at all. Read the
per-step status instead.

### An ambiguous step under a dry run

A second probe declares the same step text twice, once as a Cucumber Expression
and once as a regular expression.

```text
$ npx cucumber-js --dry-run --require ambiguous/steps.js \
    --format message:amb.ndjson ambiguous/amb.feature
EXIT=0
(no output on standard error)
```

```json
{"testCase":{"pickleId":"160f741e-...","id":"8929f5d8-...","testSteps":[
  {"id":"a5f26e5a-...","pickleStepId":"76c29e42-...",
   "stepDefinitionIds":["c84cef6b-...","50ce5d96-..."],
   "stepMatchArgumentsLists":[{"stepMatchArguments":[]},{"stepMatchArguments":[]}]}]}}
{"testStepFinished":{"testStepId":"a5f26e5a-...","testStepResult":{"status":"AMBIGUOUS","duration":{"seconds":0,"nanos":0}}}}
```

A dry run detects the conflict, reports `AMBIGUOUS`, and lists **both**
identifiers. No body runs, and the exit code is 0.

**Consequence for criv.** The derivation must handle a list of more than one
identifier. A step with two definitions has two candidate C4 elements, and the
dynamic view cannot choose one. Treat it as a drift report, not as an edge.

### What a dry run needs from the environment

A dry run needs no running application, but it does execute the top-level code
of every support file. A third probe holds a step-definition file that throws
when a database variable is absent:

```text
$ npx cucumber-js --dry-run --require env/steps.js \
    --format message:env.ndjson features/probe.feature
SIDE EFFECT: module top level ran
Error: DATABASE_URL is not set
    at Object.<anonymous> (.../env/steps.js:6:9)
    ...
    at tryRequire (.../@cucumber/cucumber/lib/try_require.js:13:16)
    at getSupportCodeLibrary (.../@cucumber/cucumber/lib/api/support.js:24:35)
```

The run gives no messages at all. The loader is unconditional:
`getSupportCodeLibrary` calls `tryRequire` for every matched path, with no
dry-run test. See
[`api/support.ts`](https://github.com/cucumber/cucumber-js/blob/17a8c39a8e9dcf87d234c11f7d0fd42d8081b65a/src/api/support.ts).

So the correct statement is narrow. A dry run needs the step-definition files
**and everything they import**, and it needs every import to succeed. It does
not need a database, a server, or a fixture.

### The cost

Three consecutive dry runs of the probe project:

```text
real 0.49
real 0.23
real 0.23
```

This is a floor, not a forecast. It measures one Node start plus two tiny
modules. A real project's cost is the cost of loading its support-code module
graph, which is **unconfirmed** and project-specific. A pre-commit hook is
plausible for a small project and is not safe to assume for a large one.

### A registry dump with no Gherkin

cucumber-js can list its step definitions with zero feature files. Point the run
at an empty directory:

```text
$ npx cucumber-js --dry-run --require features/step_definitions \
    --format message:nofeat.ndjson empty
EXIT=0
```

The output holds five envelope kinds and no pickle:

```json
{"stepDefinition":{"id":"9fbfea94-...","pattern":{"source":"a cart with one item","type":"CUCUMBER_EXPRESSION"},"sourceReference":{"uri":"features/step_definitions/cart_steps.js","location":{"line":7}}}}
{"stepDefinition":{"id":"cc0493e5-...","pattern":{"source":"the customer pays","type":"CUCUMBER_EXPRESSION"},"sourceReference":{"uri":"features/step_definitions/payment_steps.js","location":{"line":3}}}}
```

The envelope order is `meta`, `hook`, `stepDefinition`, `stepDefinition`,
`testRunStarted`, `testRunFinished`.

This is a true registry dump: pattern plus source file plus line, with no
Gherkin. It does **not** give the step-to-definition edge. Without a pickle
there is no matching, so criv would have to match the step text against the
pattern itself. Cucumber Expression matching in Rust is out of scope for
[#198](https://github.com/TudorAndrei/criv/issues/198).

### The `usage-json` formatter is a second route

cucumber-js ships a `usage-json` formatter, and the CLI help says it works with
a dry run: *"If `--dry-run` is used the duration is not shown, and step
definitions are sorted by filename instead."* Under a dry run it writes the edge
directly:

```json
[
  {
    "matches": [ { "line": 3, "text": "a cart with one item", "uri": "features/probe.feature" } ],
    "code": "function () {\n  console.error('SIDE EFFECT: cart step body ran')\n}",
    "line": 7,
    "pattern": "a cart with one item",
    "patternType": "CucumberExpression",
    "uri": "features/step_definitions/cart_steps.js"
  }
]
```

One object per step definition, with a `matches` list of feature file and line.
This needs no identifier join. Two limits matter. It keys a match by feature
file line, not by pickle or scenario identifier, so criv would have to map a
line back to a scenario itself. And it lists nothing for an undefined step,
because an undefined step has no definition to group under.

See the formatter list in
[`docs/formatters.md`](https://github.com/cucumber/cucumber-js/blob/17a8c39a8e9dcf87d234c11f7d0fd42d8081b65a/docs/formatters.md).

### The `uri` base is the process working directory

[#203](https://github.com/TudorAndrei/criv/issues/203) left the base of the
`sourceReference.uri` path unconfirmed. It is now confirmed for cucumber-js.
Running the same project from its parent directory changes the recorded path:

```text
$ cd .. && node cukejs/node_modules/.bin/cucumber-js --dry-run \
    --require cukejs/features/step_definitions \
    --format message:cukejs/cwd.ndjson cukejs/features/probe.feature
"sourceReference":{"uri":"cukejs/features/step_definitions/cart_steps.js","location":{"line":3}}
"sourceReference":{"uri":"cukejs/features/step_definitions/cart_steps.js","location":{"line":7}}
"sourceReference":{"uri":"cukejs/features/step_definitions/payment_steps.js","location":{"line":3}}
```

The path is relative to the process working directory. A consumer must know that
directory to resolve a report path against a vault path.

### The exact flag

`-d, --dry-run`, or `{ dryRun: true }` in a configuration file. See
[`argv_parser.ts` line 77](https://github.com/cucumber/cucumber-js/blob/17a8c39a8e9dcf87d234c11f7d0fd42d8081b65a/src/configuration/argv_parser.ts#L77).

## cucumber-ruby

The reviewed tree is
[`720e267`](https://github.com/cucumber/cucumber-ruby/tree/720e267cb92bcb038d3b5a62b5d9c16abd74c29f).
Behaviour was also measured on the released gem, `cucumber` 11.1.1 with
`cucumber-core` 16.2.0 and `cucumber-messages` 32.3.1, on Ruby 4.0.6. The gating
logic is the same in both trees.

### The flag

`-d, --dry-run`, described as *"Invokes formatters without executing the
steps."* See
[`cli/options.rb` line 127](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/cli/options.rb#L127)
and the repository's own
[`dry_run.feature`](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/features/docs/cli/dry_run.feature).

cucumber.io documents `dryRun` only for Java. There is no official cucumber.io
page for the Ruby flag. This is **unconfirmed** outside the repository.

### The edge is present

`Filters::ActivateSteps` searches for the match, fires `:step_activated`, and
only then tests the flag:

```ruby
return NoStepMatch.new(test_step, test_step.text) unless matches.any?
...
configuration.notify :step_activated, test_step, match
return SkippingStepMatch.new if configuration.dry_run?
```

See
[`activate_steps.rb` lines 50-60](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/filters/activate_steps.rb#L50-L60).
`:step_activated` is the event that fills the identifier map that the message
formatter reads. See
[`message_builder.rb` lines 102-107](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/formatter/message_builder.rb#L102-L107).
`SkippingStepMatch` replaces the action only, not the match record. See
[`step_match.rb` lines 99-103](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/step_match.rb#L99-L103).

Observed dry-run test step:

```json
{"id":"ad961688-...","pickleStepId":"ee2c55bf-...",
 "stepDefinitionIds":["74672f91-521b-4683-aa17-342c060e423e"],
 "stepMatchArgumentsLists":[{"stepMatchArguments":[]}]}
```

No step body printed anything.

### cucumber-ruby omits `testRunStarted`

This is the one real gap, and it differs from cucumber-js.

| Envelope | Normal run | Dry run |
| --- | --- | --- |
| `hook` | 6 | 6 |
| `stepDefinition` | 4 | 4 |
| `testCase` | 2 | 2 |
| `testRunStarted` | 1 | **0** |
| `testRunHookStarted` / `testRunHookFinished` | 2 / 2 | **0 / 0** |
| `testCaseStarted` / `testCaseFinished` | 2 / 2 | 2 / 2 |
| `testStepStarted` / `testStepFinished` | 12 / 12 | **4 / 4** |
| `suggestion` | 1 | 1 |
| `testRunFinished` | 1 | 1 |

`stepDefinition` and `hook` envelopes survive, because cucumber-ruby emits them
at registration time and not at run time. See
[`registry_and_more.rb` lines 86-92](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/glue/registry_and_more.rb#L86-L92).

`testRunStarted` disappears because its filter is added only when the flag is
absent. See
[`runtime.rb` lines 247-255](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/runtime.rb#L247-L255)
and
[`broadcast_test_run_started_event.rb`](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/filters/broadcast_test_run_started_event.rb).

**Consequence for criv.** `testCase.testRunStartedId` and
`testRunFinished.testRunStartedId` still carry an identifier that no envelope
declares. A consumer that validates references must tolerate that dangling
identifier, or it will reject every cucumber-ruby dry run. Whether this is a
filed defect or a deliberate choice is **unconfirmed**.

The `testStepStarted` count falls from 12 to 4 because hook test steps are no
longer added to the test case. The eight filters that a dry run drops are listed
together in
[`runtime.rb` lines 247-264](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/runtime.rb#L247-L264).

### Undefined and ambiguous, cucumber-ruby

An undefined step gives `UNDEFINED`, an empty `stepDefinitionIds`, and a
`suggestion` envelope. The exit code is 1, which differs from cucumber-js.

An ambiguous step gives `AMBIGUOUS`, and a dry run does detect it. The search
wrapper raises `Cucumber::Ambiguous`, and `ActivateSteps` catches it before the
flag test. See
[`step_match_search.rb` lines 14-26](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/step_match_search.rb#L14-L26)
and
[`step_match.rb` lines 145-153](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/step_match.rb#L145-L153),
where `AmbiguousStepMatch` installs an action that a dry run cannot skip.

**But `stepDefinitionIds` is empty for an ambiguous step**, because
`:step_activated` never fires. The report says a step is ambiguous and does not
say which definitions collided. Only the console error text names them. This
differs from cucumber-js, which lists both identifiers.

### cucumber-ruby loads `env.rb`

`Runtime#run!` calls `load_step_definitions` with no flag test, and the loader
requires every support file and every step-definition file. See
[`runtime.rb` line 47](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/runtime.rb#L47),
[`configuration.rb` lines 205-231](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/configuration.rb#L205-L231),
and
[`registry_and_more.rb` lines 120-126](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/glue/registry_and_more.rb#L120-L126).

Measured: a `raise` at the top of `features/support/env.rb` aborts a
`--dry-run --format message` run.

**The repository documentation is stale on this point.** Line 6 of
[`dry_run.feature`](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/features/docs/cli/dry_run.feature#L6)
still says a dry run *"omits the loading of your support/env.rb file"*. That
sentence is free text in the feature description, so no scenario tests it, and
the current code contradicts it. Do not plan against it.

### A registry dump for cucumber-ruby

cucumber-ruby has no dump subcommand, but two formatters build their list from
the `step_definition_registered` event, so they list definitions that no step
matched:

```text
$ mkdir empty
$ cucumber -r features/step_definitions -r features/support --dry-run \
    --format stepdefs empty
"a defined step"                   # features/step_definitions/steps.rb:3
  NOT MATCHED BY ANY STEPS
/^an ambiguous step$/              # features/step_definitions/steps.rb:11
  NOT MATCHED BY ANY STEPS
0 scenarios
0 steps
```

See
[`stepdefs.rb`](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/formatter/stepdefs.rb)
and
[`usage.rb` lines 29-43](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/formatter/usage.rb#L29-L43).

The machine-readable form is the `message` formatter against the same empty
directory, which gives `meta`, every `hook`, every `stepDefinition`,
`testRunStarted`, and `testRunFinished`. The `--dry-run` flag is then not needed
at all, because there is nothing to execute. The path must exist; a missing path
fails.

### `sourceReference` for cucumber-ruby

Two fields only, `uri` and `location.line`, from the block's
`Proc#source_location`. See
[`step_definition.rb` lines 78-94](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/glue/step_definition.rb#L78-L94).

It is enough to resolve one source file, with one condition. The path is
normalized against the working directory, and the normalization has three arms.
See
[`location.rb` lines 20-30](https://github.com/cucumber/cucumber-ruby-core/blob/eb33d8f74b178940cf1825060d728f94eb9112a8/lib/cucumber/core/test/location.rb#L20-L30):

1. A file under the working directory becomes a clean relative path. Resolvable.
2. A file inside a gem is cut down to `<gem>-<version>/lib/...`. **Not**
   resolvable without the gem load path.
3. Anything else keeps its absolute path.

Case 2 matters for criv. A step definition that a shared gem provides gives a
path that points outside the vault and that criv cannot map to a Source file.

## cucumber-jvm

The reviewed tree is
[`0c59737`](https://github.com/cucumber/cucumber-jvm/tree/0c59737eef646f8ce30d26ae6f3e40fb575342ba).
No run was performed; the findings are from source. Mark the JVM section as
source-read and not measured.

### The flags, and there are three

There is no single CLI flag, because the supported runner is the JUnit Platform
engine.

- The configuration parameter `cucumber.execution.dry-run=true`, whose Javadoc
  says *"When using dry run Cucumber will skip execution of glue code."* See
  [`Constants.java` lines 23-30](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/options/Constants.java#L23-L30),
  the engine's
  [re-export](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-junit-platform-engine/src/main/java/io/cucumber/junit/platform/engine/Constants.java#L30-L39),
  and
  [`CucumberConfiguration.isDryRun()`](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-junit-platform-engine/src/main/java/io/cucumber/junit/platform/engine/CucumberConfiguration.java#L156-L162).
- `@CucumberOptions(dryRun = true)` for JUnit 4. The complete annotation is
  deprecated. See
  [`CucumberOptions.java` lines 23-28](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-junit/src/main/java/io/cucumber/junit/CucumberOptions.java#L23-L28)
  and the
  [official documentation](https://github.com/cucumber/docs/blob/main/content/docs/cucumber/api.md).
- `-d, --dry-run` on `io.cucumber.core.cli.Main`, help text *"Skip execution of
  glue code."* See
  [`CommandlineOptions.java` lines 54-57](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/cli/CommandlineOptions.java#L54-L57)
  and
  [`USAGE.txt` line 51](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/resources/io/cucumber/core/options/USAGE.txt#L51).

All three set the same `RuntimeOptions.dryRun` field.

### The edge is present for a matched step

`Runner.runPickle` builds the complete test case, and only then runs it. The
matching methods do not read the flag. See
[`Runner.java` lines 162-195](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/Runner.java#L162-L195).

`TestCase.createTestStep` fills `stepDefinitionIds` from the match that already
exists:

```java
StepDefinition stepDefinition = pickleStep.getDefinitionMatch().getStepDefinition();
if (stepDefinition instanceof CoreStepDefinition coreStepDefinition) {
    stepDefinitionIds = singletonList(coreStepDefinition.getId().toString());
}
```

See
[`TestCase.java` lines 179-204](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/TestCase.java#L179-L204).

No body runs, because `ExecutionMode.DRY_RUN` calls `dryRunStep`, and
`PickleStepDefinitionMatch.dryRunStep` is empty. See
[`ExecutionMode.java` lines 15-21](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/ExecutionMode.java#L15-L21)
and
[`PickleStepDefinitionMatch.java` lines 71-74](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/PickleStepDefinitionMatch.java#L71-L74).
Parameter-type conversion does not run either, because only `runStep` converts
the arguments.

### cucumber-jvm omits no envelope, but changes the status

The flag has only three call sites in the complete main source tree:
`RuntimeOptions:160`, `Runner:108`, and `Runner:146` and `:152`. No message
emission tests it.

- `stepDefinition`, `hook`, and `parameterType` envelopes are still emitted,
  from `prepareGlue`, which `runPickle` calls for every pickle. See
  [`CachingGlue.java` lines 373-389](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/CachingGlue.java#L373-L389).
- `testStepStarted` and `testStepFinished` are still emitted, unconditionally.
- `suggestion` envelopes for an undefined step are still emitted, because the
  snippet event fires during matching. See
  [`Runner.java` lines 197-220](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/Runner.java#L197-L220).
- A matched step reports **`PASSED`**, not `SKIPPED`.
  `ExecutionMode.DRY_RUN.execute` returns `Status.PASSED`. This differs from
  cucumber-js and cucumber-ruby, which both report `SKIPPED`.
- A hook test step appears in `testCase.testSteps` with its `hookId` and gets a
  `PASSED` pair, although the body does not run.
  `HookDefinitionMatch.dryRunStep` is empty. See
  [`HookDefinitionMatch.java` lines 37-40](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/HookDefinitionMatch.java#L37-L40).

**Consequence for criv.** A per-implementation status rule is needed. `SKIPPED`
means "matched, not executed" for JavaScript and Ruby, and `PASSED` means the
same thing for Java. Neither reading is portable.

### Undefined and ambiguous, cucumber-jvm

An undefined step gives `UNDEFINED`. `UndefinedPickleStepDefinitionMatch`
throws from `dryRunStep` as well as from `runStep`. See
[`UndefinedPickleStepDefinitionMatch.java` lines 15-23](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/UndefinedPickleStepDefinitionMatch.java#L15-L23).

An ambiguous step gives `AMBIGUOUS`, and a dry run does detect it, because
`CachingGlue.findStepDefinitionMatch` throws during matching. See
[`CachingGlue.java` lines 456-472](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/CachingGlue.java#L456-L472)
and
[`AmbiguousPickleStepDefinitionsMatch.java` lines 21-29](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/AmbiguousPickleStepDefinitionsMatch.java#L21-L29).
Duplicate patterns and duplicate parameter types also throw in a dry run, from
`prepareGlue`.

**`stepDefinitionIds` is empty for both cases**, in a dry run and in a normal
run. Both use `NoStepDefinition`, which is not a `CoreStepDefinition`, so the
`instanceof` test above fails. See
[`NoStepDefinition.java`](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/NoStepDefinition.java).
For an ambiguous step, `stepMatchArgumentsLists` still holds one entry per
competing match, so the count of the conflict is visible even though the
identifiers are not.

This is a deviation from the messages documentation, which says an `AMBIGUOUS`
step has more than one entry in `stepDefinitionIds`. cucumber-js follows the
documentation. cucumber-jvm does not.

### What a dry run needs from the JVM environment

More than the other two.

- Glue must compile and be on the class path. The `Runner` constructor loads
  glue with no flag test. See
  [`Runner.java` lines 44-58](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/Runner.java#L44-L58).
- `@BeforeAll` and `@AfterAll` do **not** run. This is the one short circuit.
  See
  [`Runner.java` lines 107-124](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/Runner.java#L107-L124).
- The object-factory lifecycle **does** run, once per pickle, with no flag test.
  See
  [`Runner.java` lines 136-141](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/Runner.java#L136-L141).
- With cucumber-spring, the Spring application context **is** created and the
  glue bean **is** instantiated in a dry run. `SpringFactory.start()` builds the
  `TestContextManager`, and `TestContextAdaptor` refreshes the context and calls
  `beforeTestClass`, `createAndPrepareTestInstance`, and `beforeTestMethod`. See
  [`SpringFactory.java` lines 102-128](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-spring/src/main/java/io/cucumber/spring/SpringFactory.java#L102-L128)
  and
  [`TestContextAdaptor.java` lines 31-73](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-spring/src/main/java/io/cucumber/spring/TestContextAdaptor.java#L31-L73).

**Consequence for criv.** "A dry run needs no environment" is only true for the
plain cucumber-java backend. With a dependency-injection backend a dry run
starts the container, which means a compile plus a container start. That is not
a pre-commit cost.

### No JVM registry dump

There is no way to list the step definitions with no feature file.
`emitStepDefined` is reachable only through `prepareGlue`, and `prepareGlue` has
exactly one caller, `Runner.runPickle`. With no pickle there is no
`stepDefinition` envelope. With the JUnit Platform engine the result is
stricter: with no discovered test the engine starts nothing, plugins included.
See
[`CucumberEngineDescriptor.java` lines 35-70](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-junit-platform-engine/src/main/java/io/cucumber/junit/platform/engine/CucumberEngineDescriptor.java#L35-L70).

The plugin registry holds fourteen names, and none of them is a registry dump:
`html`, `json`, `junit`, `pretty`, `progress`, `message`, `rerun`, `summary`,
`testng`, `timeline`, `unused`, `usage`, `usage-json`, `teamcity`. See
[`PluginOption.java` lines 43-61](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/options/PluginOption.java#L43-L61).
`usage`, `usage-json`, and `unused` all consume envelopes, so they need features
too.

The closest workable trick is one dummy feature with one dummy scenario plus the
dry-run property, which makes `prepareGlue` emit the complete registry. This is
inference from the code above and is **unconfirmed** by a run.

### `sourceReference` for cucumber-jvm

Confirmed as [#203](https://github.com/TudorAndrei/criv/issues/203) reported.
Annotation-based Java glue gives a `javaMethod` object with `className`,
`methodName`, and `methodParameterTypes`, and no `uri` and no `location`. See
[`CachingGlue.java` lines 391-410](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/CachingGlue.java#L391-L410)
and
[`JavaMethodReference.java` lines 15-34](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/backend/JavaMethodReference.java#L15-L34).

One exception is new information. A cucumber-java8 lambda step definition
reports a `StackTraceElementReference` instead, and that branch **does** build a
`Location` with a line number. So "Java carries no line" holds for
annotation-based glue, not for every JVM backend.

**This is the fact that limits the derivation for Java.** A fully qualified
class name is not a file path. criv would need Java symbol knowledge to map
`com.example.CartSteps` to `src/test/java/com/example/CartSteps.java`, and the
mapping is not always the obvious one, because a nested class, a source root
that is not `src/test/java`, and a class in a different file all break it.

## Go, Python, and Rust

[#203](https://github.com/TudorAndrei/criv/issues/203) reported that these three
emit no messages report, and
[#198](https://github.com/TudorAndrei/criv/issues/198) records that as a limit.
That is still true of the messages format, but the conclusion drawn from it is
too strong. **Four of the five runners do give a machine-readable
step-to-definition edge, in their own format.** One of them emits real messages.

All findings here are from source at these commits:

| Runner | Reviewed commit |
| --- | --- |
| godog | [`17b50c6`](https://github.com/cucumber/godog/tree/17b50c6f76ed8ef2d604f169dc8827912f6ef14f) |
| behave | [`a842816`](https://github.com/behave/behave/tree/a842816f3c67ee55fbb932b5a96e3b2305512747) |
| pytest-bdd | [`c17dbb2`](https://github.com/pytest-dev/pytest-bdd/tree/c17dbb2de74bebd4d16cfc865773286460e06e34) |
| pytest-bdd-ng | [`859d32b`](https://github.com/elchupanebrej/pytest-bdd-ng/tree/859d32bc3f2f107d4c7c07f112b7ed5bcb665a10) |
| cucumber-rs | [`d958725`](https://github.com/cucumber-rs/cucumber/tree/d95872541ee2a43d71089c64bd3fa8848e24c922) |

### Summary

| Runner | Dry run | Machine-readable edge | What the edge holds |
| --- | --- | --- | --- |
| godog | none | `--format=events`, NDJSON | Base file name, line, and the package-qualified function name |
| behave | `-d`, `--dry-run` | `--dry-run -f json` | Relative path and line of the step function |
| pytest-bdd | none | **none** | `match.location` is the empty string |
| pytest-bdd-ng | none | `--messages-ndjson` | A real `StepDefinition` with a `file://` URI and a line |
| cucumber-rs | none | `writer::Libtest`, `--format=json --show-output` | `path:line:col (matched)` inside a text field |

### behave gives a dry run and an edge

behave is the closest of the five to the cucumber-js result. The flag is `-d` or
`--dry-run`. See
[`configuration.py` line 121](https://github.com/behave/behave/blob/a842816f3c67ee55fbb932b5a96e3b2305512747/behave/configuration.py#L121).

A dry run still matches every step against the registry, then hands the real
`Match` to every formatter:

```python
found_step_match = runner.step_registry.find_match(step)
...
elif dry_run_scenario:
    for formatter in runner.formatters:
        # -- EMULATE: Step.run() protocol w/o step execution.
        formatter.match(found_step_match)
        formatter.result(step)
```

See
[`model.py` line 1219](https://github.com/behave/behave/blob/a842816f3c67ee55fbb932b5a96e3b2305512747/behave/model.py#L1219).

`behave --dry-run -f json` therefore writes both ends. The step's own `location`
is the feature file, and the nested `match.location` is the step definition, as
a relative path plus a line from `func.__code__`:

```python
match_data = {
    "location": str(match.location) or "",
    "arguments": args,
}
if match.location:
    # -- NOTE: match.location=None occurs for undefined steps.
```

See
[`formatter/json.py` line 141](https://github.com/behave/behave/blob/a842816f3c67ee55fbb932b5a96e3b2305512747/behave/formatter/json.py#L141)
and
[`model_type.py` line 380](https://github.com/behave/behave/blob/a842816f3c67ee55fbb932b5a96e3b2305512747/behave/model_type.py#L380).
An undefined step gets no `match` key at all.

behave also has registry dumps, but they are text only and there is no JSON
variant. `steps.usage` is the only one that prints both ends. `steps.catalog`
deliberately prints no location, because it sets `shows_location = False`. See
[`formatter/steps.py` line 430](https://github.com/behave/behave/blob/a842816f3c67ee55fbb932b5a96e3b2305512747/behave/formatter/steps.py#L430)
and
[`formatter/steps.py` line 305](https://github.com/behave/behave/blob/a842816f3c67ee55fbb932b5a96e3b2305512747/behave/formatter/steps.py#L305).

behave refuses to start with no feature file at all. It raises
`ConfigError('No feature files in %r')`. See
[`runner.py` line 1070](https://github.com/behave/behave/blob/a842816f3c67ee55fbb932b5a96e3b2305512747/behave/runner.py#L1070).
behave's own tests add an empty feature file to work around this. So a registry
dump with no Gherkin is not available.

### godog has no dry run, and two usable formatters

A search for `dry` over the complete tree gives no result.
[`internal/flags/options.go`](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/internal/flags/options.go#L17)
has no such field.

godog has a true registry dump instead. `-d`, `--definitions` prints every
available step definition, and it is handled before feature-path resolution, so
it needs no feature file. See
[`internal/flags/flags.go` line 40](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/internal/flags/flags.go#L40)
and
[`run.go` line 219](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/run.go#L219).
It exits with code 2, because it returns `exitOptionError`. See
[`run.go` line 31](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/run.go#L31).

The best machine-readable edge is `--format=events`, which is NDJSON and gives
one `StepDefinitionFound` object per matched step, with both ends:

```json
{"event":"StepDefinitionFound","location":"formatter-tests/features/single_scenario_with_passing_step.feature:7","definition_id":"fmt_output_test.go:101 -> github.com/cucumber/godog/internal/formatters_test.passingStepDef","arguments":[]}
```

That is a committed golden fixture. See
[`fmt_events.go` line 247](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/internal/formatters/fmt_events.go#L247)
and
[`formatter-tests/events/single_scenario_with_passing_step`](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/internal/formatters/formatter-tests/events/single_scenario_with_passing_step).

The legacy `cucumber` JSON formatter does populate `match.location`, and it does
hold the step-definition location, not the feature file. See
[`fmt_cucumber.go` line 293](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/internal/formatters/fmt_cucumber.go#L293).
Three traps go with it:

1. **The path is a base file name with no directory**, because `DefinitionID`
   calls `filepath.Base`. The golden fixture holds
   `"match": {"location": "fmt_output_test.go:101"}`. See
   [`internal/formatters/fmt.go` line 101](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/internal/formatters/fmt.go#L101).
2. **The line is the registration call site, not the handler body.** godog takes
   `runtime.Caller(2)` when `ctx.Step` runs. See
   [`test_context.go` line 350](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/test_context.go#L350).
3. **An undefined, a pending, and an ambiguous step overwrite the field with the
   feature path.** A consumer must read `result.status` to know which kind of
   location it holds. See
   [`fmt_cucumber.go` line 302](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/internal/formatters/fmt_cucumber.go#L302).

godog imports `cucumber/messages/go/v34` only as a library, for the `Pickle` and
`GherkinDocument` types. It has five formatters and none of them emits messages.
No third-party messages formatter was found. A third party would also be
blocked, because the public
[`formatters.StepDefinition`](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/formatters/fmt.go#L95)
holds `Expr`, `Handler`, and `Keyword` and no `File` or `Line`.

### pytest-bdd has no route at all

This is the one hard negative. `match.location` is hard coded to the empty
string:

```python
return {
    "keyword": step["keyword"],
    "name": step_name,
    "line": step["line_number"],
    "match": {"location": ""},
    "result": self._get_result(step, report, error_message),
}
```

See
[`cucumber_json.py` line 169](https://github.com/pytest-dev/pytest-bdd/blob/c17dbb2de74bebd4d16cfc865773286460e06e34/src/pytest_bdd/cucumber_json.py#L169).
It is a deliberate contract: the test suite asserts the empty string in seven
places. See
[`tests/feature/test_cucumber_json.py` line 134](https://github.com/pytest-dev/pytest-bdd/blob/c17dbb2de74bebd4d16cfc865773286460e06e34/tests/feature/test_cucumber_json.py#L134).

pytest-bdd never computes a step-definition location. `StepFunctionContext`
holds the function object and no location. See
[`steps.py` line 65](https://github.com/pytest-dev/pytest-bdd/blob/c17dbb2de74bebd4d16cfc865773286460e06e34/src/pytest_bdd/steps.py#L65).
The step-to-function edge is a fixture-name lookup that is resolved lazily
during execution and then torn down. See
[`scenario.py` line 168](https://github.com/pytest-dev/pytest-bdd/blob/c17dbb2de74bebd4d16cfc865773286460e06e34/src/pytest_bdd/scenario.py#L168).

There is no dry run, and `--collect-only` gives scenarios only. The plugin
registers no collection hook, so no step hook fires, and a node identifier names
the scenario. See
[`plugin.py` line 57](https://github.com/pytest-dev/pytest-bdd/blob/c17dbb2de74bebd4d16cfc865773286460e06e34/src/pytest_bdd/plugin.py#L57)
and
[`scenario.py` line 433](https://github.com/pytest-dev/pytest-bdd/blob/c17dbb2de74bebd4d16cfc865773286460e06e34/src/pytest_bdd/scenario.py#L433).
Scenario Outline steps do not even have final text until run time.

### pytest-bdd-ng emits real messages

This corrects the earlier report.
[#203](https://github.com/TudorAndrei/criv/issues/203) noted that pytest-bdd-ng
emits envelopes. It also gives the complete edge.

The flag is `--messages-ndjson PATH`. See
[`entrypoint.py` line 229](https://github.com/elchupanebrej/pytest-bdd-ng/blob/859d32bc3f2f107d4c7c07f112b7ed5bcb665a10/src/pytest_bdd/plugin/gherkin_message_reporter/entrypoint.py#L229).
It writes one envelope per line and validates against bundled JSON schemas.

`StepDefinition` carries a real Python location:

```python
source_reference=SourceReference(
    uri=Path(resolvepath(source_file, getattr(config, "rootpath", Path.cwd()))).as_uri(),
    location=Location(line=source_line, column=1),
    java_method=JavaMethod(...),
)
```

See
[`steps/definition.py` line 136](https://github.com/elchupanebrej/pytest-bdd-ng/blob/859d32bc3f2f107d4c7c07f112b7ed5bcb665a10/src/pytest_bdd/steps/definition.py#L136).
`TestStep` carries `stepDefinitionIds`. See
[`step_catalog_runtime.py` line 142](https://github.com/elchupanebrej/pytest-bdd-ng/blob/859d32bc3f2f107d4c7c07f112b7ed5bcb665a10/src/pytest_bdd/plugin/gherkin_message_reporter/step_catalog_runtime.py#L142).

Three caveats:

- **No dry run.** The messages come from a `pytest_runtest_setup` wrapper hook
  after the yield, so the setup phase must run. See
  [`step_catalog_runtime.py` line 75](https://github.com/elchupanebrej/pytest-bdd-ng/blob/859d32bc3f2f107d4c7c07f112b7ed5bcb665a10/src/pytest_bdd/plugin/gherkin_message_reporter/step_catalog_runtime.py#L75).
- The `uri` is a `file://` URI, not a relative path. This differs from
  cucumber-js and cucumber-ruby.
- The line is the first decorator line, not the `def` line.
- Every parser flavour is downgraded to `REGULAR_EXPRESSION` for schema
  compatibility. See
  [`message_serialization.py` line 20](https://github.com/elchupanebrej/pytest-bdd-ng/blob/859d32bc3f2f107d4c7c07f112b7ed5bcb665a10/src/pytest_bdd/model/message_serialization.py#L20).

pytest-bdd-ng is a third-party project and not a cucumber one. Depending on it
would make criv's Python support depend on a single maintainer.

### cucumber-rs knows the location but hides it

There is no dry run. A search for `dry` over the `.rs`, `.md`, and `.toml` files
gives no result. The complete default CLI is in
[`book/src/cli.md`](https://github.com/cucumber-rs/cucumber/blob/d95872541ee2a43d71089c64bd3fa8848e24c922/book/src/cli.md).

The good news is that cucumber-rs **does** record the step function location.
The `#[given]`, `#[when]`, and `#[then]` macros expand `file!()`, `line!()`, and
`column!()`:

```rust
loc: ::cucumber::step::Location {
    path: ::std::file!(),
    line: ::std::line!(),
    column: ::std::column!(),
},
```

See
[`codegen/src/attribute.rs` line 135](https://github.com/cucumber-rs/cucumber/blob/d95872541ee2a43d71089c64bd3fa8848e24c922/codegen/src/attribute.rs#L135)
and
[`src/step.rs` line 256](https://github.com/cucumber-rs/cucumber/blob/d95872541ee2a43d71089c64bd3fa8848e24c922/src/step.rs#L256).
The location is the attribute line, not the `fn` line, and it flows through
every step event.

The bad news is the writers.

- **`writer::Json` has no `match` field at all.** Its `Step` struct holds
  `keyword`, `line`, `name`, `hidden`, `result`, and `embeddings`. See
  [`src/writer/json.rs` line 509](https://github.com/cucumber-rs/cucumber/blob/d95872541ee2a43d71089c64bd3fa8848e24c922/src/writer/json.rs#L509).
  The location appears only for a **failed** step, as free text inside
  `error_message`. A passed step sets `error_message: None` and loses it. See
  [`src/writer/json.rs` line 306](https://github.com/cucumber-rs/cucumber/blob/d95872541ee2a43d71089c64bd3fa8848e24c922/src/writer/json.rs#L306).
- `writer::Basic` prints `Matched: <path>:<line>:<col>` for a failed step only,
  as terminal text. See
  [`src/writer/basic.rs` line 708](https://github.com/cucumber-rs/cucumber/blob/d95872541ee2a43d71089c64bd3fa8848e24c922/src/writer/basic.rs#L708).
- `writer::JUnit` never uses the location.

The one machine-readable route that covers a passed step is `writer::Libtest`
with `--format=json --show-output`, behind the `libtest` cargo feature. The flag
documentation says what it is for: *"Show captured stdout of successful tests.
Currently, outputs only step function location."* See
[`src/writer/libtest.rs` line 46](https://github.com/cucumber-rs/cucumber/blob/d95872541ee2a43d71089c64bd3fa8848e24c922/src/writer/libtest.rs#L46)
and
[`src/writer/libtest.rs` line 603](https://github.com/cucumber-rs/cucumber/blob/d95872541ee2a43d71089c64bd3fa8848e24c922/src/writer/libtest.rs#L603).

A committed fixture shows the shape:

```json
{"type":"test","event":"failed","name":"Feature: Basic tests/features/wait/rule.feature::19: Rule: rule::21: Scenario: 2 secs::24:  Then 2 secs","stdout":"tests/features/wait/rule.feature:24:7 (defined)\ntests/libtest.rs:9:1 (matched)\nStep panicked. ..."}
```

See
[`tests/libtest/correct.stdout` line 19](https://github.com/cucumber-rs/cucumber/blob/d95872541ee2a43d71089c64bd3fa8848e24c922/tests/libtest/correct.stdout#L19).

It is NDJSON, but the edge lives inside a free-text `stdout` string that a
consumer must parse for `(defined)` and `(matched)`. It needs a real test run,
because there is no dry run. The `Writer` trait is a public extension point and
`step::Location` is public, so a third party **could** write a messages writer.
A search of crates.io found none today. The nearest neighbours are
`cucumber-reporter`, which writes HTML, and `cuke-dedup`, which finds duplicate
step definitions statically.

### The effect on criv's own vault

criv is a Rust project. If criv ever grew a `.feature` file of its own, the only
route to the edge would be `writer::Libtest` with `--show-output`, which needs a
real test run and gives the edge inside a text field. That is the same conflict
that [#208](https://github.com/TudorAndrei/criv/issues/208) already owns.

## What this means for the derivation

### A dry run is a real plan phase, and that is the useful finding

For the three implementations that emit messages, matching is a separate phase
that runs before execution and does not read the dry-run flag. This is not an
accident of one release; it is the same structure in all three:

- cucumber-js assembles the test case in `src/assemble/`, which holds no
  reference to `dryRun`.
- cucumber-ruby fires `:step_activated` before it tests `dry_run?`.
- cucumber-jvm builds the complete `TestCase` in `Runner.runPickle` before it
  calls `testCase.run(bus)`.

So the answer to the ticket's question is yes. A dry run gives the
step-to-definition edge with no step body executed.

### criv must still run cucumber, and that is the real obstacle

[[0046-no-native-linting-in-criv-enforce|ADR-0046]] keeps language tools out of
`criv enforce`, and
[#198](https://github.com/TudorAndrei/criv/issues/198) lists "criv runs
cucumber, or any other test runner" as out of scope. A dry run is a cheaper run,
not a non-run. It starts a process, loads the complete support-code module
graph, and executes every top-level statement in it.

So the finding does not remove the ADR-0046 conflict. It changes the size of the
conflict, not its kind. The decision that
[#208](https://github.com/TudorAndrei/criv/issues/208) owns is unchanged: either
criv reads a report that somebody else produced, or it does not read a report.

### Four facts that limit the derivation

1. **Java gives a class and a method, not a file.** criv cannot resolve
   `com.example.CartSteps#aCartWithOneItem` to a Source file without Java symbol
   knowledge. Any derivation that needs a file for the `link ... 'source'`
   lookup fails for annotation-based Java glue. A cucumber-java8 lambda is the
   exception and does carry a line.
2. **Every report path is relative to the working directory of the run**, and no
   implementation records that directory. cucumber-ruby additionally rewrites a
   gem path to `<gem>-<version>/lib/...`, which points outside the vault.
   pytest-bdd-ng writes a `file://` URI instead. criv needs a per-report base
   path input, and cannot infer it.
3. **An ambiguous step gives no identifiers except in cucumber-js.**
   cucumber-ruby and cucumber-jvm both report `AMBIGUOUS` with an empty
   `stepDefinitionIds`, which deviates from the messages documentation. A
   derivation must treat an ambiguous step as drift, not as an edge, and cannot
   name the competing definitions from the report.
4. **The status of a matched step is not portable.** cucumber-js and
   cucumber-ruby report `SKIPPED`. cucumber-jvm reports `PASSED`. A consumer
   that filters on status needs a per-implementation rule, and `meta` carries the
   implementation name it would key on.

Only fact 1 makes a derivation impossible for a language. The other three make
it need inputs that a report alone does not carry.

### One route is much simpler than the messages report

If criv ever does read a dry-run artifact, `usage-json` deserves a look before
the messages NDJSON. It gives one object per step definition with a `matches`
list of feature file and line, which needs no four-link identifier join. It is
cucumber-js and cucumber-jvm only; cucumber-ruby has `usage` but no
`usage-json`. This is a shape observation, not a recommendation.

## Reproducing the experiment

The scratch project is outside the repository and is not committed. To rebuild
it:

1. `npm install @cucumber/cucumber` in an empty directory.
2. Write one feature with three steps, and two step-definition files that
   implement the first two. Make every body and every hook write a marker line
   to standard error.
3. Run `npx cucumber-js --format message:normal.ndjson <feature>`, then
   `npx cucumber-js --dry-run --format message:dry.ndjson <feature>`.
4. Count the envelope keys of each NDJSON line in both files, and compare.
