# Cucumber Messages Report Contract Research

Date: 2026-09-13

Issue: [#203](https://github.com/TudorAndrei/criv/issues/203).
Map: [#198](https://github.com/TudorAndrei/criv/issues/198).

## Question

What is the contract of the cucumber messages report, and can one consumer read
it across the JavaScript, Ruby, Java, Go, Python, and Rust implementations?

## Answer

The contract is stable, versioned, and well specified. One consumer can read it,
but only for three of the six implementations that the ticket names.

- **JavaScript, Ruby, and Java emit the format.** The flag is the `message`
  formatter or plugin in all three.
- **Go, Python, and Rust do not emit the format.** godog, behave, pytest-bdd,
  and cucumber-rs have no `message` writer. They emit the legacy Cucumber JSON
  format, or their own format. One third-party Python runner, pytest-bdd-ng,
  does emit envelopes.
- A consumer that reads the format works the same for all three emitters, with
  one exception. `sourceReference` holds a file path and a line for JavaScript
  and Ruby, but holds a Java class and method for Java. A step-to-file-and-line
  edge is therefore not portable.
- No envelope holds a scenario result. The consumer must collect the step
  results of a test case and select the most severe one.

The reviewed version is cucumber messages `34.2.1`, released 2026-08-05. The
reviewed commit is
[`b9b8315`](https://github.com/cucumber/messages/tree/b9b8315eb7564ca350da5d4227dc02a192ec100c).

## The encoding

A message stream is NDJSON. One line holds one JSON object. That object is
always an `Envelope`. See the official
[Cucumber Messages README](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/README.md#encoding).

The design goal is a stream. The producer writes each message when the event
happens, and the consumer can process the stream before the run stops. See the
[reason for the format](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/README.md#high-memory-footprint).

## The envelope types

`Envelope` holds 21 optional fields. Each field holds one message type. No field
is required. See the
[Envelope schema](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/src/Envelope.schema.json)
and the
[Envelope documentation](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#envelope).

| Field | Purpose |
| --- | --- |
| `meta` | Protocol version, implementation, runtime, OS, CPU, CI data. |
| `source` | The complete text of one source file. |
| `gherkinDocument` | The abstract syntax tree of one parsed source file. |
| `parseError` | A Gherkin parse failure for one source file. |
| `pickle` | One executable scenario, derived from the syntax tree. |
| `stepDefinition` | One step definition, with its pattern and its source location. |
| `parameterType` | One custom parameter type. |
| `undefinedParameterType` | A parameter type that an expression uses but does not declare. |
| `hook` | One hook, with its type and its source location. |
| `suggestion` | Code snippets that could implement one undefined step. |
| `testRunStarted` | The start of the run. |
| `testRunFinished` | The end of the run, with a `success` boolean. |
| `testRunHookStarted` | The start of one global hook execution. |
| `testRunHookFinished` | The end of one global hook execution, with a result. |
| `testCase` | The execution plan of one pickle, as a list of test steps. |
| `testCaseStarted` | The start of one attempt of one test case. |
| `testCaseFinished` | The end of one attempt, with a `willBeRetried` boolean. |
| `testStepStarted` | The start of one test step. |
| `testStepFinished` | The end of one test step, **with its result**. |
| `attachment` | An embedded attachment, such as a screenshot. |
| `externalAttachment` | An attachment that a URL holds, added in `32.0.0`. |

### Which envelope holds a scenario result

`testStepFinished` is the only envelope that holds a step result.
`testRunHookFinished` holds a result for a global hook. No other envelope holds
a result. See
[`TestStepFinished`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#teststepfinished).

`testCaseFinished` holds only `testCaseStartedId`, `timestamp`, and
`willBeRetried`. It holds **no result**. See
[`TestCaseFinished`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#testcasefinished).

Therefore a consumer must compute the scenario result. Collect every
`testStepFinished` that has the same `testCaseStartedId`, then select the most
severe status. The official helper defines this severity order, from least
severe to most severe:

```text
UNKNOWN < PASSED < SKIPPED < PENDING < UNDEFINED < AMBIGUOUS < FAILED
```

See
[`getWorstTestStepResult.ts`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/javascript/src/getWorstTestStepResult.ts).

`testRunFinished.success` gives the result of the complete run. The
documentation says a run is successful when all steps passed or were skipped,
all hooks passed, and no other exception occurred. See
[`TestRunFinished.success`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#testrunfinishedsuccess).

## The step statuses

`TestStepResult.status` is a closed enumeration of seven values. The schema
lists them in this order:

1. `UNKNOWN`
2. `PASSED`
3. `SKIPPED`
4. `PENDING`
5. `UNDEFINED`
6. `AMBIGUOUS`
7. `FAILED`

See the
[`TestStepResult` schema](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/src/TestStepResult.schema.json)
and the
[`TestStepResultStatus` documentation](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#teststepresultstatus).

The schema order is also the severity order that `getWorstTestStepResult` uses.

`UNKNOWN` is a sentinel. The compatibility kit `all-statuses` sample produces
the other six statuses and does not produce `UNKNOWN`. The official helper
returns `UNKNOWN` only when it receives an empty list of results. See the
[`all-statuses` sample](https://github.com/cucumber/compatibility-kit/blob/49fc3c0d7fcdf7e40de560770b035e3d259c7305/devkit/samples/all-statuses/all-statuses.ndjson).

The compatibility kit states the run-level effect of `UNDEFINED`. The step is
`UNDEFINED`, every step after it in the same scenario is `SKIPPED`, and the run
fails. See the
[`undefined` feature](https://github.com/cucumber/compatibility-kit/blob/49fc3c0d7fcdf7e40de560770b035e3d259c7305/devkit/samples/undefined/undefined.feature).

`TestStepResult` also holds an optional `message` string and an optional
`exception` object. `exception` holds `type`, `message`, and `stackTrace`. See
[`Exception`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#exception).

## The step-to-code edge

The edge exists, but it is not portable.

### The chain of identifiers

The chain has four links, and every link uses a string identifier:

1. `pickle.steps[].id` gives the identifier of a pickle step.
2. `testCase.testSteps[]` holds `pickleStepId` and `stepDefinitionIds`.
3. `stepDefinition.id` matches one entry of `stepDefinitionIds`.
4. `stepDefinition.sourceReference` points at the code.

See
[`TestStep`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#teststep)
and
[`StepDefinition`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#stepdefinition).

`stepDefinitionIds` is a list, and the documentation states its cardinality:

- An `UNDEFINED` step has an empty `stepDefinitionIds` list.
- An `AMBIGUOUS` step has more than one entry.
- A matched step has exactly one entry.

See the
[`TestStep` description](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#teststep).

`stepDefinitionIds` is optional, and it is absent when the test step comes from
a hook and not from a pickle step. A hook test step holds `hookId` instead.

### SourceReference is a union, and each language uses a different arm

`SourceReference` has four optional fields: `uri`, `javaMethod`,
`javaStackTraceElement`, and `location`. All four are optional, so an empty
`SourceReference` is valid. See the
[`SourceReference` documentation](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#sourcereference).

JavaScript writes `uri` and `location.line`. It writes no column. See
[`emit_support_code_messages.ts`](https://github.com/cucumber/cucumber-js/blob/f3f489faadc9b310533b9e5b3879bd30239e24c1/src/api/emit_support_code_messages.ts#L48-L55).

Ruby writes `uri` from the file of the definition, and `location.line` from the
first line of the definition. It writes no column. See
[`step_definition.rb`](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/glue/step_definition.rb#L86-L91).

Java writes neither `uri` nor a portable line. `createSourceReference` has three
branches:

- A `JavaMethodReference` becomes a `javaMethod` object, which holds
  `className`, `methodName`, and `methodParameterTypes`. **It carries no line
  and no file.**
- A `StackTraceElementReference` becomes a `javaStackTraceElement` plus a
  `location`. The `fileName` is a bare file name and not a path, and the code
  substitutes the literal string `"Unknown"` when the file name is absent.
- Any other reference becomes `new SourceReference(null, null, null, null)`,
  which is an empty object.

See
[`CachingGlue.java`](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/src/main/java/io/cucumber/core/runner/CachingGlue.java#L391-L414).

The Java branch has a source comment that asks for a schema fix, so this is a
known limit and not a defect of one release.

### Conclusion on the edge

For JavaScript and Ruby, `stepDefinitionIds` plus `sourceReference` give a
usable step-to-file-and-line edge. The line is the line of the step definition,
and there is no column.

For Java, the same pair gives a step-to-class-and-method edge. A consumer must
resolve the class and the method to a file itself, and criv would need Java
symbol knowledge to do that. A file-and-line edge is therefore **not** available
for Java from the report alone.

The `uri` value is a relative path in the JavaScript and Ruby samples, for
example `samples/minimal/minimal.ts`. Whether the base of that path is always
the working directory of the run is **unconfirmed**. Both implementations copy a
path that their own loader produced, and neither documents the base.

## How a pickle maps back to Gherkin

`Pickle` holds `uri`, `name`, an optional `location`, and a required
`astNodeIds` list. See the
[`Pickle` documentation](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#pickle).

### The scenario name

`pickle.astNodeIds` holds the identifiers of the syntax tree nodes of the
pickle. For a plain scenario it holds one identifier, and that identifier is
`scenario.id`. The `rules` sample shows this:

```json
{"pickle":{"id":"22","name":"Not enough money","location":{"line":9,"column":5},"astNodeIds":["4"], ...}}
```

`4` is the `scenario.id` of `Example: Not enough money`. See the
[`rules` sample](https://github.com/cucumber/compatibility-kit/blob/49fc3c0d7fcdf7e40de560770b035e3d259c7305/devkit/samples/rules/rules.ndjson).

`pickle.name` is **not** always equal to `scenario.name`. For a Scenario Outline
with a parameterized name, the pickle name has the placeholder replaced. The
`examples-tables` sample has the scenario name
`Eating cucumbers with <friends> friends` and the pickle names
`Eating cucumbers with 11 friends`, `Eating cucumbers with 1 friends`, and
`Eating cucumbers with 0 friends`. See the
[`examples-tables` feature](https://github.com/cucumber/compatibility-kit/blob/49fc3c0d7fcdf7e40de560770b035e3d259c7305/devkit/samples/examples-tables/examples-tables.feature)
and its
[messages](https://github.com/cucumber/compatibility-kit/blob/49fc3c0d7fcdf7e40de560770b035e3d259c7305/devkit/samples/examples-tables/examples-tables.ndjson).

To get the scenario name, a consumer must read `astNodeIds[0]` and find that
scenario in the `gherkinDocument`. `pickle.name` alone is not the scenario name.

### The Examples row

For a pickle that an `Examples` table produced, `astNodeIds` holds two
identifiers. The first is the `Scenario` node, and the second is the `TableRow`
node. The documentation states this, and the sample confirms it:

```json
{"pickle":{"id":"26","name":"Eating cucumbers","location":{"line":19,"column":7},"astNodeIds":["13","4"]}}
{"pickle":{"id":"30","name":"Eating cucumbers","location":{"line":20,"column":7},"astNodeIds":["13","5"]}}
```

Scenario `13` is `Eating cucumbers`. Rows `4` and `5` are the two body rows of
`Examples: These are passing`. See
[`Pickle.astNodeIds`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#pickleastnodeids).

`pickle.location` points at the example row and not at the scenario keyword. The
documentation says so, and line 19 in the sample is the first body row.

One limit is important. `astNodeIds` holds the `TableRow` identifier, but it does
**not** hold the `Examples` block identifier. To name the `Examples` block, a
consumer must walk the `gherkinDocument`, read `scenario.examples[]`, and find
the block whose `tableBody[]` contains that row identifier. In the sample,
`Examples: These are passing` has id `7` and holds rows `4` and `5`, and
`Examples: These are failing` has id `12` and holds rows `9` and `10`. Neither
`7` nor `12` appears in any `astNodeIds`.

### The Rule

`Pickle` has **no** `ruleId` field, and the Rule identifier does **not** appear
in `astNodeIds`. The `rules` sample proves this: the three pickles carry only
`["4"]`, `["9"]`, and `["15"]`, which are scenario identifiers, while the Rule
identifiers are `10` and `17`.

To find the Rule, a consumer must walk the `gherkinDocument`. The structure is
`feature.children[]`, where each `FeatureChild` holds an optional `rule`, an
optional `background`, or an optional `scenario`. A `Rule` holds
`children[]` of `RuleChild`, and each `RuleChild` holds an optional `background`
or an optional `scenario`. A scenario is inside a Rule when its identifier
appears under `rule.children[].scenario.id`. See
[`FeatureChild`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#featurechild)
and
[`RuleChild`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#rulechild).

Rule tags are inherited into `pickle.tags`, and each `PickleTag` holds the
`astNodeId` of its `Tag` node. In the `rules` sample, pickle `32` carries
`[{"name":"@some-tag","astNodeId":"16"}]`, and node `16` is the tag of the second
Rule. A tag is therefore an indirect route to the Rule, but only when the Rule
has a tag. The tree walk is the reliable route.

### Consequence for the criv scenario identity

Issue [#200](https://github.com/TudorAndrei/criv/issues/200) decided that a
scenario identity is the qualified name in kebab case, for example
`features/checkout.feature#scenario:place-an-order`. Three facts of this
contract touch that decision:

1. The report always gives `pickle.uri`, so the file part of the identity is
   available.
2. For a Scenario Outline, `pickle.name` is the expanded name and is not the
   scenario name. A consumer that builds the identity from `pickle.name` would
   produce a different identity than a consumer that reads the `.feature` file.
   The identity must come from `astNodeIds[0]` and the `gherkinDocument`.
3. One scenario produces many pickles when it has an `Examples` table. A
   scenario identity therefore maps to a **set** of results, not to one result.

## Which implementations emit the format

The official README names the emitters. It says that cucumber messages are
currently sent by these versions, and that emitters for the other languages are
not yet implemented. See the
[Message emitters section](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/README.md#message-emitters).

| Implementation | Emits messages | Exact flag | Since |
| --- | --- | --- | --- |
| cucumber-js (JavaScript) | Yes | `--format message` for stdout, or `--format "message":"out.ndjson"` for a file | `7.0.0` |
| cucumber-ruby (Ruby) | Yes | `--format message --out report.ndjson`. There is no `message:file` colon form. | `4.0.0` |
| cucumber-jvm (Java) | Yes | `--plugin message:report.ndjson` or `-p message:report.ndjson` on the CLI, `@CucumberOptions(plugin = {"message:report.ndjson"})`, or the `cucumber.plugin=message:report.ndjson` property | `6.0.0` |
| godog (Go) | No | none | not applicable |
| behave (Python) | No | none | not applicable |
| pytest-bdd (Python) | No | `--cucumberjson=<path>` writes legacy Cucumber JSON, not messages | not applicable |
| cucumber-rs (Rust) | No | none | not applicable |
| pytest-bdd-ng (Python, third party) | Yes | `--messages-ndjson=PATH` | `2.0.0` |
| cucumber-node (JavaScript) | Yes | Listed as a compatibility kit adopter | unconfirmed |
| Reqnroll (.NET) | Yes | Not researched; the README names it | `3.0.0` |

The messages library version that each emitter depends on is close to current.
cucumber-js depends on `@cucumber/messages` `34.2.1` on `main`. cucumber-jvm
depends on `io.cucumber:messages` `34.2.1` on `main` and on `30.1.0` at the
released `v7.34.8`. cucumber-ruby depends on `cucumber-core`, which requires
`cucumber-messages` in the range `> 31, < 35`. So a report from a released
cucumber-jvm can declare a protocol version several major releases behind a
report from cucumber-js, and a criv reader must tolerate that spread.

### JavaScript

The `message` formatter is built in. The official documentation says it "Outputs
all the Cucumber Messages for the test run as newline-delimited JSON, which can
then be consumed by other tools". See
[`docs/formatters.md`](https://github.com/cucumber/cucumber-js/blob/f3f489faadc9b310533b9e5b3879bd30239e24c1/docs/formatters.md#message).

The same document gives the syntax. A format takes one or two values. Without a
second value the formatter prints to stdout. On the command line, the name and
the path use `:` as a delimiter, and each side needs double quotes. In a
configuration file, use `{ format: [['message', 'out.ndjson']] }`. See the
[top of `docs/formatters.md`](https://github.com/cucumber/cucumber-js/blob/f3f489faadc9b310533b9e5b3879bd30239e24c1/docs/formatters.md).

The same document also says that the legacy `json` formatter is in maintenance
mode and that `message` is the recommended structured output.

The formatter itself is one statement. It writes
`` write(`${JSON.stringify(message)}\n`) `` for each `message` event, so the
output is exactly the envelope objects with no wrapper. See
[`src/formatter/builtin/message.ts`](https://github.com/cucumber/cucumber-js/blob/f3f489faadc9b310533b9e5b3879bd30239e24c1/src/formatter/builtin/message.ts).
The formatter first shipped in `7.0.0`.

### Ruby

`message` is a built-in format, described as "Prints each message in NDJSON
form, which can then be consumed by other tools". The `json` entry in the same
table says the JSON format is in maintenance mode and recommends the message
formatter. See
[`lib/cucumber/cli/options.rb`](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/cli/options.rb#L27-L32)
and
[`lib/cucumber/formatter/message.rb`](https://github.com/cucumber/cucumber-ruby/blob/720e267cb92bcb038d3b5a62b5d9c16abd74c29f/lib/cucumber/formatter/message.rb).

Ruby differs from Java in the flag shape. Ruby names the formatter with
`--format` or `-f`, and it takes the output path from a separate `--out` or
`-o` flag. There is no `message:file` colon form.

### Java

`message` is a built-in plugin, described as "Logs cucumbers execution as a
stream of json messages". See the
[`cucumber-core` README plugin list](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/README.md#built-in-plugins).

The plugin is selected with the `cucumber.plugin` property, which takes comma
separated plugin strings in the form `name:path`. Cucumber reads the property
from system properties, environment variables, `@CucumberOptions`, and
`cucumber.properties`, in that order of precedence, and CLI arguments take
precedence over all of them. See the
[`cucumber-core` README properties section](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-core/README.md#properties-environment-variables-system-options).

With the JUnit Platform engine, the same property is passed as a system
property, for example `-Dcucumber.plugin=message:report.ndjson`. See the
[JUnit Platform engine README](https://github.com/cucumber/cucumber-jvm/blob/0c59737eef646f8ce30d26ae6f3e40fb575342ba/cucumber-junit-platform-engine/README.md).

### Go

godog has no `message` formatter. Its formatter directory registers `cucumber`,
`events`, `junit`, `pretty`, and `progress`. See the
[formatters directory](https://github.com/cucumber/godog/tree/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/internal/formatters).

The `cucumber` formatter produces the legacy Cucumber JSON format. Its own source
comment points at the old relishapp JSON output formatter documentation. See
[`fmt_cucumber.go`](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/internal/formatters/fmt_cucumber.go).

The `events` formatter is a different NDJSON stream. Its registration string is
"Produces JSON event stream, based on spec: 0.1.0". That `0.1.0` is a godog
specific number and is not a cucumber messages protocol version. See
[`fmt_events.go`](https://github.com/cucumber/godog/blob/17b50c6f76ed8ef2d604f169dc8827912f6ef14f/internal/formatters/fmt_events.go).

godog does depend on `github.com/cucumber/messages/go/v34`, and it uses the
message types for its internal Gherkin model. Emitting the message stream would
therefore be a small change for godog, but it does not do it today. The request
is tracked as an open issue,
[godog#341 "Implement message formatter"](https://github.com/cucumber/godog/issues/341).
godog also has no `compatibility` directory, so it does not run the
compatibility kit.

### Python

behave has no `message` formatter. Its formatter package holds `json`, `null`,
`plain`, `pretty`, `progress`, `rerun`, `steps`, `tags`, and a few others. See
the
[behave formatter package](https://github.com/behave/behave/tree/a842816f3c67ee55fbb932b5a96e3b2305512747/behave/formatter).

behave's `json` formatter is behave's own format, not cucumber messages and not
the legacy Cucumber JSON format.

The compatibility kit does publish a Python package, but that package only
distributes the reference test data for a Python consumer. It is not a Python
cucumber implementation. See the
[compatibility kit Python README](https://github.com/cucumber/compatibility-kit/blob/49fc3c0d7fcdf7e40de560770b035e3d259c7305/python/README.md).

pytest-bdd emits the legacy Cucumber JSON format with `--cucumberjson=<path>`,
not messages. See
[`cucumber_json.py`](https://github.com/pytest-dev/pytest-bdd/blob/master/src/pytest_bdd/cucumber_json.py).
radish does the same with `--cucumber-json=<path>`. See
[`cucumber_json_writer.py`](https://github.com/radish-bdd/radish/blob/master/radish/extensions/cucumber_json_writer.py).

There is one Python runner that emits envelopes, and it is a third-party fork of
pytest-bdd. pytest-bdd-ng has a `--messages-ndjson=PATH` option and depends on
the official `cucumber-messages` package. It is not in the `cucumber`
organization. See
[the reporter entry point](https://github.com/elchupanebrej/pytest-bdd-ng/blob/default/src/pytest_bdd/plugin/gherkin_message_reporter/entrypoint.py)
and its
[changelog](https://github.com/elchupanebrej/pytest-bdd-ng/blob/default/CHANGES.md).

An official Python **library** for the message types does exist, published as
the `cucumber-messages` package from the same repository. It is a reader and a
type set, not an emitter. See
[`python/pyproject.toml`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/python/pyproject.toml).

### Rust

cucumber-rs has no `message` writer. Its writer module holds `basic`, `discard`,
`fail_on_skipped`, `json`, `junit`, `libtest`, `normalize`, `or`, `out`,
`repeat`, `summarize`, and `tee`. See the
[writer directory](https://github.com/cucumber-rs/cucumber/tree/d95872541ee2a43d71089c64bd3fa8848e24c922/src/writer).

Its `json` writer is the legacy Cucumber JSON format. Its own documentation
comment links to `cucumber/cucumber-json-schema`. See
[`src/writer/json.rs`](https://github.com/cucumber-rs/cucumber/blob/d95872541ee2a43d71089c64bd3fa8848e24c922/src/writer/json.rs).

There is also no first-party Rust binding for cucumber messages. The
`cucumber/messages` repository has language directories for C++, Dart, .NET,
Elixir, Go, Java, JavaScript, Perl, PHP, Python, and Ruby, and no Rust
directory. See the
[repository root](https://github.com/cucumber/messages/tree/b9b8315eb7564ca350da5d4227dc02a192ec100c).
A crates.io search for a cucumber messages crate returned no such crate.

A Rust consumer must therefore define its own serde types from the JSON schema,
or generate them.

## Version and ordering guarantees

### The schema is versioned

`Meta.protocolVersion` is required, and the documentation says it is the SemVer
version number of the protocol. See
[`Meta.protocolVersion`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#metaprotocolversion).

The protocol version is the version of the `cucumber/messages` release. The
repository changelog states that the project follows SemVer, and its most recent
entry is `34.2.1`, dated 2026-08-05. See the
[changelog](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/CHANGELOG.md).

The implementations copy the version of their messages library into the field.
cucumber-js writes `protocolVersion: messagesVersion`, where `messagesVersion`
is the `version` export of `@cucumber/messages`. See
[`emit_support_code_messages.ts`](https://github.com/cucumber/cucumber-js/blob/f3f489faadc9b310533b9e5b3879bd30239e24c1/src/api/emit_support_code_messages.ts#L21-L46).
The Java library reads it from a resource bundle at
[`ProtocolVersion.java`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/java/src/main/java/io/cucumber/messages/ProtocolVersion.java).

A real `meta` envelope from the compatibility kit shows the value:

```json
{"meta":{"protocolVersion":"34.2.1","implementation":{"name":"fake-cucumber","version":"123.45.6"}, ...}}
```

### How a consumer must behave on an unknown version

The project does not publish a required consumer behaviour for an unknown
version. This is **unconfirmed** as a written rule. What the sources do give is a
clear de-facto contract:

The forward-compatibility rule is on the producer side. The contributing guide
tells schema authors to add new fields last and to never add a new field as
`required`, "this will make the new code unable to read existing messages". See
[`CONTRIBUTING.md`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/CONTRIBUTING.md).

The official JavaScript reader does no version check and no validation at all.
`parseEnvelope` is `JSON.parse(json) as Envelope`. See
[`parseEnvelope.ts`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/javascript/src/parseEnvelope.ts).

So the working rule for a consumer is:

1. Do not reject a stream on an unknown `protocolVersion`. Nothing in the
   protocol asks for that, and the reference reader does not do it.
2. Ignore unknown fields, and ignore unknown envelope keys.
3. Treat every field as optional unless the schema marks it required, because
   an older producer will omit newer fields.
4. Do not strict-validate against a pinned schema. Every schema file sets
   `"additionalProperties": false`, so an old schema **rejects** a newer valid
   envelope. Strict validation against a pinned schema is a forward-compatibility
   trap. See the
   [`Envelope` schema](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/src/Envelope.schema.json).
5. Treat a new enumeration value as unknown rather than as an error. Status is a
   closed enumeration today, and the schema author could add a value in a major
   release.

A consumer should also expect the enumeration to be stable. `TestStepResultStatus`
has not changed in the reviewed changelog history.

### Ordering is only a partial order

The compatibility kit states the ordering contract. Messages are only partially
ordered. Scenarios that run in parallel may interleave their messages, and
feature files may be parsed in parallel. Some orderings are not specified at
all, and even `TestRunStarted` is not guaranteed to be the first message. The
guarantees are:

- A `*Started` message comes before its matching `*Finished` message.
- `TestCaseStarted` and `TestCaseFinished` are inside
  `TestRunStarted`/`TestRunFinished`.
- `TestStepStarted` and `TestStepFinished` are inside
  `TestCaseStarted`/`TestCaseFinished`.
- `Attachment` messages are inside `TestStepStarted`/`TestStepFinished`.
- Before a message is emitted, every message it refers to should already have
  been emitted.

See the "Ordering" section of the
[compatibility kit README](https://github.com/cucumber/compatibility-kit/blob/49fc3c0d7fcdf7e40de560770b035e3d259c7305/README.md).

A consumer must therefore index by identifier and must not rely on line order.

### The compatibility kit is the reference

The Cucumber Compatibility Kit holds reference `.ndjson` files for 46 feature
samples. A given feature, run with its step definitions, must emit the matching
messages. cucumber-js, cucumber-node, cucumber-jvm, cucumber-ruby, and Reqnroll
all use the kit. See the
[compatibility kit README](https://github.com/cucumber/compatibility-kit/blob/49fc3c0d7fcdf7e40de560770b035e3d259c7305/README.md).

This is what makes one consumer possible across the three emitters. The kit is
also the best test fixture source for a criv reader, because the files are real
output and are version controlled.

## Example envelopes

All example lines below are verbatim from the compatibility kit, which generates
them with `fake-cucumber`, the reference implementation in the kit devkit.

### A passing scenario

The `minimal` sample has one scenario with one step. The complete stream is 12
lines, in this order: `meta`, `source`, `gherkinDocument`, `pickle`,
`stepDefinition`, `testRunStarted`, `testCase`, `testCaseStarted`,
`testStepStarted`, `testStepFinished`, `testCaseFinished`, `testRunFinished`.
See the
[`minimal` sample](https://github.com/cucumber/compatibility-kit/blob/49fc3c0d7fcdf7e40de560770b035e3d259c7305/devkit/samples/minimal/minimal.ndjson).

The pickle:

```json
{"pickle":{"id":"3","uri":"samples/minimal/minimal.feature","location":{"line":9,"column":3},"astNodeIds":["1"],"tags":[],"name":"cukes","language":"en","steps":[{"id":"2","text":"I have 42 cukes in my belly","type":"Context","astNodeIds":["0"]}]}}
```

The step definition, which carries the code location:

```json
{"stepDefinition":{"id":"4","pattern":{"type":"CUCUMBER_EXPRESSION","source":"I have {int} cukes in my belly"},"sourceReference":{"uri":"samples/minimal/minimal.ts","location":{"line":3}}}}
```

The test case, which joins pickle step `2` to step definition `4`:

```json
{"testCase":{"id":"6","pickleId":"3","testSteps":[{"id":"7","pickleStepId":"2","stepDefinitionIds":["4"],"stepMatchArgumentsLists":[{"stepMatchArguments":[{"group":{"start":7,"value":"42"},"parameterTypeName":"int"}]}]}],"testRunStartedId":"5"}}
```

The result, which is the only line that holds a status:

```json
{"testStepFinished":{"testCaseStartedId":"8","testStepId":"7","testStepResult":{"status":"PASSED","duration":{"seconds":0,"nanos":1000000}},"timestamp":{"seconds":0,"nanos":3000000}}}
```

The scenario end, which holds no status:

```json
{"testCaseFinished":{"testCaseStartedId":"8","timestamp":{"seconds":0,"nanos":4000000},"willBeRetried":false}}
```

### An undefined step

The `undefined` sample has four scenarios. See the
[`undefined` sample](https://github.com/cucumber/compatibility-kit/blob/49fc3c0d7fcdf7e40de560770b035e3d259c7305/devkit/samples/undefined/undefined.ndjson).

The test case has an **empty** `stepDefinitionIds` list, which is how a consumer
detects an unmatched step before the run even starts:

```json
{"testCase":{"id":"23","pickleId":"11","testSteps":[{"id":"24","pickleStepId":"10","stepDefinitionIds":[],"stepMatchArgumentsLists":[]}],"testRunStartedId":"22"}}
```

The result:

```json
{"testStepFinished":{"testCaseStartedId":"33","testStepId":"24","testStepResult":{"status":"UNDEFINED","duration":{"seconds":0,"nanos":0}},"timestamp":{"seconds":0,"nanos":3000000}}}
```

A `suggestion` envelope arrives between the `testStepStarted` and the
`testStepFinished`, and it gives the snippet that would implement the step:

```json
{"suggestion":{"id":"34","pickleStepId":"10","snippets":[{"language":"typescript","code":"Given(\"a step that is yet to be defined\", () => {\n  return \"pending\"\n})"}]}}
```

The run then fails:

```json
{"testRunFinished":{"testRunStartedId":"22","timestamp":{"seconds":0,"nanos":21000000},"success":false}}
```

`Suggestion` and `Snippet` were added to the protocol for this purpose. See
[`Suggestion`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#suggestion).
This makes the report a direct answer to the "documented behaviour with no code"
case, because it names the pickle step and offers the code.

## What this means for criv

criv would be a consumer of this format, and ADR-0046 keeps criv out of the
runner business. These facts follow from the contract.

1. **One reader works, for three languages.** The envelope shape, the status
   enumeration, and the identifier graph are identical for cucumber-js,
   cucumber-ruby, and cucumber-jvm, and the compatibility kit enforces that. A
   criv reader needs no per-implementation branch, except for
   `sourceReference`.
2. **Rust has no first-party binding.** criv must define its own serde types.
   The JSON schema is bundled as one file since messages `32.1.0`, so generation
   is possible.
3. **Scenario status is a computation, not a field.** criv must group
   `testStepFinished` by `testCaseStartedId` and take the most severe status.
   The severity order is published.
4. **A step-to-code edge is portable only for file plus line, and only for
   JavaScript and Ruby.** For Java, criv would receive a class and a method.
   Building `criv query callers` from this report would work unevenly across
   languages.
5. **A scenario maps to a set of results.** A Scenario Outline produces one
   pickle per Examples row, so a criv scenario symbol has many results, and a
   mixed outcome is normal.
6. **The Rule and the Examples name need the `gherkinDocument`.** A criv reader
   that only reads the result envelopes cannot reconstruct a Rule or an Examples
   block name. It must also keep the `gherkinDocument` envelope, or re-parse the
   `.feature` file.

### Facts that affect the freshness contract in issue 208

Issue [#208](https://github.com/TudorAndrei/criv/issues/208) must decide what
criv reports when a report is absent, old, or from a different revision. These
contract facts constrain that design.

1. **The report can carry a git revision, but only from CI.** `Meta.ci.git`
   holds `remote`, `revision`, and optionally `branch` and `tag`. The
   compatibility kit `meta` sample shows a real one. But `Meta.ci` is
   **optional**, and the `@cucumber/ci-environment` library returns a value only
   when it detects a CI server. See the
   [ci-environment README](https://github.com/cucumber/ci-environment/blob/main/README.md)
   and [`Meta.ci`](https://github.com/cucumber/messages/blob/b9b8315eb7564ca350da5d4227dc02a192ec100c/jsonschema/messages.md#metaci).
   A revision match is therefore available for a CI-produced report and is
   **not** available for a developer's local run. A freshness rule that needs a
   revision will work in CI and will degrade to a file timestamp locally.
2. **The report carries no source content hash.** There is no field that holds a
   digest of the `.feature` file or of the step definition file. `Source.data`
   holds the complete feature text, so criv could hash `Source.data` itself and
   compare it to the working tree. That is a stronger staleness signal than the
   git revision, because it works locally too, and it is per file.
3. **`TestRunStarted.timestamp` gives the run time.** It is required, so every
   report has a run time that does not depend on the file modification time.
4. **The `UNDEFINED` signal is available without running anything past parse.**
   `testCase.testSteps[].stepDefinitionIds` is empty for an unmatched step. So
   "a document describes behaviour with no code" is visible in the plan and not
   only in the result.
5. **A retried scenario produces more than one attempt.** `TestCaseStarted`
   holds `attempt`, and `TestCaseFinished` holds `willBeRetried`. criv must
   pick the last attempt, or it will report a stale failure for a scenario that
   later passed.
6. **Parallel runs interleave.** criv must not read the report as an ordered
   log, and must not assume that the last `testStepFinished` in the file belongs
   to the last scenario.
7. **Partial reports are normal.** An aborted run produces a stream with no
   `testRunFinished`. A consumer sees this as a missing envelope, not as an
   error. A freshness rule needs a definition of an incomplete report.

## Sources

All facts above cite a first-party source. The primary sources are:

- [`cucumber/messages`](https://github.com/cucumber/messages/tree/b9b8315eb7564ca350da5d4227dc02a192ec100c)
  at `b9b8315`, version `34.2.1`. The JSON schema and its generated
  documentation are the protocol.
- [`cucumber/compatibility-kit`](https://github.com/cucumber/compatibility-kit/tree/49fc3c0d7fcdf7e40de560770b035e3d259c7305)
  at `49fc3c0`. The reference message streams and the ordering contract.
- [`cucumber/cucumber-js`](https://github.com/cucumber/cucumber-js/tree/f3f489faadc9b310533b9e5b3879bd30239e24c1)
  at `f3f489f`.
- [`cucumber/cucumber-ruby`](https://github.com/cucumber/cucumber-ruby/tree/720e267cb92bcb038d3b5a62b5d9c16abd74c29f)
  at `720e267`.
- [`cucumber/cucumber-jvm`](https://github.com/cucumber/cucumber-jvm/tree/0c59737eef646f8ce30d26ae6f3e40fb575342ba)
  at `0c59737`.
- [`cucumber/godog`](https://github.com/cucumber/godog/tree/17b50c6f76ed8ef2d604f169dc8827912f6ef14f)
  at `17b50c6`.
- [`cucumber-rs/cucumber`](https://github.com/cucumber-rs/cucumber/tree/d95872541ee2a43d71089c64bd3fa8848e24c922)
  at `d958725`.
- [`behave/behave`](https://github.com/behave/behave/tree/a842816f3c67ee55fbb932b5a96e3b2305512747)
  at `a842816`.
- [`pytest-dev/pytest-bdd`](https://github.com/pytest-dev/pytest-bdd),
  [`radish-bdd/radish`](https://github.com/radish-bdd/radish), and
  [`elchupanebrej/pytest-bdd-ng`](https://github.com/elchupanebrej/pytest-bdd-ng),
  for the other Python options. These were read on their default branch and are
  not pinned.
- [`cucumber/ci-environment`](https://github.com/cucumber/ci-environment), for
  the `Meta.ci` behaviour. Read on its default branch and not pinned.

## Unconfirmed items

- The base directory of `SourceReference.uri`. Neither cucumber-js nor
  cucumber-ruby documents whether the path is relative to the working directory
  of the run.
- Any written rule for a consumer that meets an unknown `protocolVersion`. The
  de-facto contract above is inferred from the reference reader and from the
  contributing guide, not from a published statement.
- The exact flag for cucumber-node. The compatibility kit names it as an
  adopter, but its own documentation was not reviewed.
- Whether pytest-bdd-ng `2.4.0` is released. The changelog has the section, but
  the newest GitHub release tag is `pytest-bdd-ng-2.3.1`.
- Whether the behave maintainers intend to add message support. No issue or
  roadmap entry was checked. The conclusion rests on the built-in formatter
  list only.
- Whether the cucumber-rs maintainers intend to add a message writer. No
  maintainer statement was found. The conclusion rests on `Cargo.toml` and the
  writer module only.
- Whether the legacy `json` formatter will be removed. Three implementations say
  it is in maintenance mode, and none gives a removal release.
