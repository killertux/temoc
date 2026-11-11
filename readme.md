
# Temoc

Acceptance tests in (GitHub Flavored) markdown files. 

## Why?

The idea is so you are able to write simple documentations with accepatance tests, commit it together with your code and validate it in your CI pipeline. Markdown files are universal, easy to write and understand so they are perfect as a medium to have your acceptance tests.

We are using the GitHub Flavored markdown beacuse we need support for [tables](https://github.github.com/gfm/#tables-extension-).

## Why not [Fitnesse](https://fitnesse.org/)?

This project is heavily inspired on fitnesse. We even use the same Slim Protocol to communicate with the SUT(System Under Test). The idea is not to compete with fitnesse, but simply be an alternative. If you want a more feature rich software, you probably should use Fitnesse.

## How to test my project?

We use the [Slim Protocol](https://fitnesse.org/FitNesse/UserGuide/WritingAcceptanceTests/SliM/SlimProtocol.html) to talk with the system under test. [Here](https://fitnesse.org/PlugIns.html) you can find a list of plugins for it in multiple languages. This repository also contains our [SliM V0.5 implementation for Rust](https://github.com/killertux/temoc/tree/master/rust_slim).

Temoc preserves recursive SliM result values when a Markdown table stores and
later compares a symbol, so nested lists returned by `callAndAssign` remain
structured instead of being flattened to strings.

You will use one of these plugins to write the test fixtures in your project. Fixtures are glue code that serves as intermediary between Temoc and your software.

After this is done, you can write your accepatance tests markdown. Take a look at our calculator example [Here](https://github.com/killertux/temoc/tree/master/temoc/examples). See the raw markdown files because there are hidden instructions. To run it, you can run `cargo run -- -c Config.toml.example`.

## Installing Temoc

On Linux and macOS (x86_64 or aarch64), you can install the latest release with:

```sh
curl -fsSL https://raw.githubusercontent.com/killertux/temoc/master/install.sh | sh
```

This downloads the right binary for your system from [GitHub Releases](https://github.com/killertux/temoc/releases) and installs it to `~/.local/bin`. Set `TEMOC_INSTALL_DIR` to install it somewhere else, or `TEMOC_VERSION` to pin a release:

```sh
curl -fsSL https://raw.githubusercontent.com/killertux/temoc/master/install.sh | TEMOC_VERSION=0.9.6 TEMOC_INSTALL_DIR=/usr/local/bin sh
```

## Building Temoc

Temoc is written in rust, you can install the rust toolchain [here](https://rustup.rs/). After that, you can compile everything using `cargo build --release`. This should create a compiled binary in `./target/release/temoc`.

## Running Temoc

Once you have compiled it, you can run `./temoc --help` to see the list of commands. Basically you will need to specify a command to start the slim server (your plugin should help you on how to do this), a port to be used in the connection, and a list of markdown files to test. Port `1` uses the V0.5 stdin/stdout transport; any other port uses TCP. You can also write a configuration file to have a default list of parameters, you can look at an example [here](https://github.com/killertux/temoc/tree/master/Config.toml.example)

Here is an example of the output of running Temoc for the [Calculator Example](https://github.com/killertux/temoc/tree/master/temoc/examples)

```

Testing file temoc/examples/calculator.md...OK
Testing file temoc/examples/calculator_2.md...SNOOZED

```

To run part of a file, add a line number to its path. The line of a `decisionTable` directive runs that table, and the line of a table row runs only that row, together with the table's `Make`, `beginTable` and `endTable`. A row run alone can't see symbols set by other rows.

## Persistent server

By default Temoc starts the slim server once for each file, so that every file begins with a clean process. When starting the server is expensive, `--persistent-server`, or `persistent_server = true` in the configuration file, starts it once and uses it for every file. The server starts when the first file with something to run needs it, and gets its `bye` when the run ends.

This is plain Slim: instruction lists repeat on one connection until `bye`. What changes is that nothing is cleaned up between files anymore, so whatever the server keeps is still there for the next file:

- Static and global state of your fixtures and of the system under test.
- Symbols set with `$NAME=`, when used in a later setter cell. Temoc itself still forgets them at the end of the file, so a `$NAME` in an output cell of another file is an error.
- Paths added with `import`.

Use [file hooks](#file-hooks) to put the server back into a known state between files.

If the connection is lost or the server dies, the run stops, as it does without this option.

## File hooks

> This is a Temoc extension. It is not part of FitNesse or of the Slim protocol, although it only uses standard Slim instructions, so it works with any Slim server.

`--file-hooks <Class>`, or `file_hooks = "Class"` in the configuration file, names a fixture that Temoc calls around every file that has something to run:

1. A new instance of the fixture is made, with no arguments.
2. `beginFile(path)` is called, with the path of the file as it was given to Temoc.
3. The tables of the file run.
4. `endFile()` is called, on the same instance.

```rust
#[fixture]
impl SpecHooks {
    pub fn begin_file(&mut self, path: String) {
        self.transaction = Some(database::begin());
    }

    pub fn end_file(&mut self) {
        if let Some(transaction) = self.transaction.take() {
            transaction.rollback();
        }
    }
}
```

The rules:

- Both methods are optional, like `beginTable` and `endTable`. A method that the fixture doesn't have is skipped.
- The fixture is made before the `import`s of the file run, so name the class in a way that needs no import, or rely on an import of an earlier file when the server is persistent.
- If the fixture can't be made, the tables of the file don't run and neither do the hooks. The file fails.
- If `beginFile` fails, the tables of the file don't run and the file fails. `endFile` still runs.
- `endFile` runs whenever the fixture was made, even after a file that stopped early with `ABORT_SLIM_TEST` or `ABORT_SLIM_SUITE`. It doesn't run when the connection was lost or the run was cancelled.
- A failed `endFile` fails the file.
- `beginFile` can raise the control exceptions: `IGNORE_ALL_TESTS` and `ABORT_SLIM_TEST` skip the file, `ABORT_SLIM_SUITE` stops the run.
- Each part is sent as its own instruction list, so the hooks add three round trips to a file.
- A file selected with `file.md:LINE` gets its hooks too. A file with nothing to run gets none.

The hooks belong to no line of the file, so failures are reported at line 0: `in file.md:0:0 for method call beginFile` in text output, and a `setup_failed` event with `"line":0` and the phase `make`, `begin_file` or `end_file` in JSONL output.

## Machine-readable output

With `--output jsonl`, or `output = "jsonl"` in the configuration file, Temoc prints one JSON object per line while the tests run. Tools can use it to show each row as it finishes. The default is `--output text`.

```jsonl
{"type":"run_started","schema":1,"temoc_version":"0.10.0"}
{"type":"file_started","file":"a.md","tables":[{"line":7,"class":"Calculator","columns":["a","b","sum?"],"rows":[11],"snoozed_until":null}]}
{"type":"table_started","file":"a.md","line":7}
{"type":"row_started","file":"a.md","table":7,"line":11}
{"type":"row_finished","file":"a.md","table":7,"line":11,"status":"failed","duration_ms":2,"checks":[{"role":"setter","method":"setA","column":3,"status":"passed","value":"1"},{"role":"setter","method":"setB","column":7,"status":"passed","value":"2"},{"role":"getter","method":"sum","column":11,"status":"failed","expected":"4","actual":{"kind":"value","value":"3"}}]}
{"type":"table_finished","file":"a.md","line":7,"status":"failed","duration_ms":3,"counts":{"failed":1}}
{"type":"file_finished","file":"a.md","status":"failed","duration_ms":9,"counts":{"failed":1}}
{"type":"run_finished","status":"failed","exit_code":1,"duration_ms":40,"counts":{"failed":1}}
```

### What you can rely on

- Every stdout line is one complete JSON object, flushed as it is written. Nothing else goes to stdout. Server output is only reported through `output` events.
- `run_finished` is the last line, unless Temoc is killed outright, so a missing `run_finished` means a crash. Invalid command line arguments are the exception: they print a usage error on stderr and exit with 2, before any event.
- The events of a file come in order, and a `row_finished` comes before the next `row_started`.
- Every row listed in `file_started` gets one `row_finished`.
- The exit code is 0 when nothing really failed and 1 otherwise, including runs with a `run_error` or a cancelled run. Snoozed failures and ignored tests don't fail a run.
- All lines and columns are 1-based and match the markdown source. Paths are as given on the command line, without the line selector.
- Consumers must ignore unknown fields and event types. Adding them doesn't change `schema`. Removing or changing something does.

### Events

| Event | Fields |
| ----- | ------ |
| `run_started` | `schema`, `temoc_version` |
| `file_started` | `file`, `tables`: a list with `line` (of the directive), `class`, `columns`, `rows` (their lines), `snoozed_until` |
| `table_started` | `file`, `line`. Not sent for a table that never runs. |
| `row_started` | `file`, `table`, `line`. Not sent for a row that never runs. |
| `row_finished` | `file`, `table`, `line`, `status`, `duration_ms`, `checks`, and `message` if the connection was lost during the row |
| `table_finished` | `file`, `line`, `status`, `duration_ms`, `counts` |
| `file_finished` | `file`, `status`, `duration_ms`, `counts` |
| `setup_failed` | `file`, `line`, `phase`, `status`, `actual`, `snoozed_until` |
| `run_error` | `kind`, `message`, `file`, `stderr` (the end of the server's stderr, if any) |
| `output` | `stream` (`stdout` or `stderr`), `text`, and on a best effort basis `file`, `table`, `row`. Only with `-o`. |
| `run_finished` | `status`, `exit_code`, `duration_ms`, `counts` |

`counts` has the number of rows for each status, leaving out zeros.

### Statuses

| Status | Meaning |
| ------ | ------- |
| `passed` | Everything matched. |
| `failed` | A wrong value. |
| `error` | An exception, an unknown symbol, or anything else that isn't a wrong value. |
| `snoozed` | A failure or error under a snooze that is still active. |
| `skipped` | It never ran. |
| `cancelled` | Only on `run_finished`: Temoc got SIGTERM or SIGINT, or stdout was closed. |

A row, table, file or run takes its worst status, in this order: `error`, `failed`, `snoozed`, `skipped`, `passed`.

### Checks

Each item of `checks` has `role`, `method`, `status`, and usually `column`.

| Field | Meaning |
| ----- | ------- |
| `role` | `setter`, `getter`, `method` (a cell of a `#method` table), `reset` or `execute`. `reset` and `execute` only appear when they fail, and have no `column`. |
| `value` | The source text of a setter's cell. |
| `expected` | The source text of a getter's cell. |
| `resolved` | What a `$SYMBOL` cell stood for, in the same format as `actual`. |
| `assigned` | The symbol set by a `$V=` cell. `actual` has the value. |
| `actual` | What the server returned. Passing setters leave it out. |
| `snoozed_until` | The date of the table's snooze, on any check that didn't pass. An expired snooze is a normal `failed` that keeps the date. It doesn't depend on `--show-snoozed`. |

`actual` is one of:

```json
{"kind":"value","value":"4"}
{"kind":"void"}
{"kind":"list","value":[{"kind":"value","value":"a"}]}
{"kind":"exception","message":"boom","detail":"the full text from the server"}
{"kind":"missing_symbol","symbol":"NOPE"}
{"kind":"protocol_error","message":"Different ID in response"}
```

A `#method` row is compared cell by cell. If the server's answer isn't a list with one element for each cell, there is one check for the whole row with no `column`.

A setter cell that is exactly `$NAME`, for a symbol that isn't set, is a `missing_symbol` error and that row isn't sent. This is stricter than text output, which passes the text on to the fixture.

### Failures outside a row

`setup_failed` reports a failed `import`, `make`, `begin_table` or `end_table` as its `phase`. After a failed `make`, the rows of that table are `skipped`.

With [file hooks](#file-hooks), the `phase` can also be `begin_file` or `end_file`, and a `make` can be the one of the hooks fixture. These three have `"line":0`, and after a failed `make` or `begin_file` every row of the file is `skipped`.

`run_error` reports problems with the run itself:

| `kind` | Meaning | What happens next |
| ------ | ------- | ----------------- |
| `no_match` | A `file:LINE` selector matched no table or row. | The next path runs. |
| `parse_error` | A markdown file couldn't be read as tests. | The next path runs. |
| `file_error` | A path couldn't be read. | The next path runs. |
| `bad_config` | The configuration is invalid. | Nothing runs. |
| `server_start_failed` | The server didn't start. | The run stops. |
| `server_died` | The connection to the server was lost. | The run stops. |
| `protocol_error` | The server sent something that can't be understood. | The run stops. |
| `suite_aborted` | A fixture raised `ABORT_SLIM_SUITE`. | The run stops. |

When the run stops, the rows that are left in the current file are `skipped`, and later files are not reported at all.

### Control exceptions

| Exception | Status of the check | What is skipped |
| --------- | ------------------- | --------------- |
| `ABORT_SLIM_TEST` | `error` | The rest of the file, except `endFile` |
| `ABORT_SLIM_SUITE` | `error` | The rest of the run, except `endFile` of the current file |
| `IGNORE_SCRIPT_TEST` | `skipped` | The rest of the table |
| `IGNORE_ALL_TESTS` | `skipped` | The rest of the file |

### Batches

Text output sends a whole file to the server in one batch. JSONL output sends one batch for `Import`, `Make` and `beginTable`, one for each row, and one for `endTable`, on the same connection. This is how the Slim protocol works: instruction lists repeat until `bye`, and FitNesse itself sends one list per table.

In both formats, [file hooks](#file-hooks) add a batch for making the fixture, one for `beginFile` and one for `endFile`, and a [persistent server](#persistent-server) keeps the connection open from one file to the next.

### Cancelling

On SIGTERM or SIGINT, Temoc stops sending, stops the server, reports the remaining rows as `skipped`, and ends with a `run_finished` with the status `cancelled`. The server runs in its own process group, so that Temoc can stop what it started: it gets SIGTERM, and SIGKILL after two seconds. Because of that, signal Temoc itself and let it stop the server.
