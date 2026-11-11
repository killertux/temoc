# Temoc reference

What Temoc actually does, for writing specs and fixtures. Matches Temoc 0.11.

## A spec file

GitHub Flavored Markdown. Directives are link reference definitions, which
render as nothing:

```markdown
## Late fees

An invoice unpaid after its due date gets a fee of 2% per started week,
capped at 10%.

[//]: # (import Billing.Fixtures)
[//]: # (decisionTable LateFeeFixture)

| amount | days overdue | # why             | late fee? |
| ------ | ------------ | ----------------- | --------- |
| 100.00 | 0            | on time           | 0.00      |
| 100.00 | 1            | first week starts | 2.00      |
| 100.00 | 8            | second week       | 4.00      |
| 100.00 | 60           | capped            | 10.00     |
```

Both `[//]: # (...)` and `[//]: # "..."` work.

| Directive | Effect |
| --------- | ------ |
| `import <path>` | Adds a path/namespace to the fixture search path, so later tables can use the short class name. |
| `decisionTable <Class>` | The **next block must be a table**; it is run against fixture `<Class>`. Anything else in between is a parse error. |
| `decisionTable <Class>#<method>` | Single-method table (below). |
| `... -- snooze until YYYY-MM-DD` | Suffix on either form. Failures in this table do not fail the run through that date. |

A table with no directive in front of it is plain documentation and is not run.

## Headers

| Header | Meaning |
| ------ | ------- |
| `days overdue` | Input. Calls `setDaysOverdue(cell)`. Words are joined in camelCase; a header already starting with `set` is used as is. |
| `late fee?` | Output. Calls `lateFee()` and compares the return with the cell. Text after the `?` is ignored. |
| `# anything` | Comment column. Never sent to the fixture. |

Cell text is the plain text of the cell: emphasis, links and inline code are
stripped to their text. Every row must have as many cells as the header.

## Execution order

For each table: construct the fixture, `beginTable()`, then for every row
`reset()`, the setters left to right, `execute()`, the outputs left to right,
and finally `endTable()`. `beginTable`, `reset`, `execute` and `endTable` are
optional: leave them out of a fixture that does not need them.

Typical use: `reset` clears per-row state, `execute` performs the action once
after all inputs are set (so the output methods are plain reads), `beginTable`
and `endTable` set up and tear down what the table shares.

Each table gets a new fixture instance, and files run one after another.
State that several tables in a file must share therefore lives in the system
under test (for example its database) or in process-level state in the
fixture server.

## Server lifetime and file hooks

By default each **file** gets a new server process, which is the isolation
between files. With `persistent_server = true` (or `--persistent-server`) the
server starts once for the whole run. Use it when startup is expensive
(booting an application, opening connection pools). Nothing is reset between
files then: fixture statics, server-side symbols and imports carry over, so
the fixtures must restore a known state themselves.

That is what file hooks are for. `file_hooks = "SpecHooks"` (or
`--file-hooks SpecHooks`) names a fixture that surrounds every file: a new
instance is made, `beginFile(path)` is called, the tables run, then
`endFile()` is called on the same instance. Both methods are optional. This
is a Temoc extension, not FitNesse.

- If the fixture can't be made or `beginFile` fails, the tables of the file
  are skipped and the file fails.
- `endFile` runs whenever the fixture was made, also after a file that
  aborted. It does not run if the server died or the run was cancelled, so
  `beginFile` should also be able to clean up what a crashed run left.
- The fixture is made before the file's `import`s; name the class so that it
  needs none.
- Hook failures are reported at line 0 of the file.

Typical use: open a database transaction in `beginFile` and roll it back in
`endFile`, so the tables of a file share state and the next file starts clean.

## Single-method tables

`decisionTable Fixture#method` calls `method(input cells...)` once per row,
with the input cells as arguments in column order, and expects a list back
with one element per `?` column. Useful when the inputs only make sense
together. `reset` still runs before each row; setters and `execute` are not
called.

## Comparison

Exact text equality, no trimming of the returned value, no numeric tolerance,
no ranges or regexes. Format in the fixture. An expected cell of `OK` or
`VOID` matches those SLIM results.

## Symbols

| Cell | In an output column | In an input column |
| ---- | ------------------- | ------------------ |
| `$NAME=` | Stores the returned value in `NAME` (passes unless the call throws). | n/a |
| `$NAME` | Expects the return to equal the stored value. | Passes the stored value to the setter. |

Symbols live for one file. Use them to carry generated values (ids, tokens)
between rows and tables without writing them in the spec. A row run on its
own cannot see symbols set by other rows. Output cells of a single-method
table are compared as plain text, so `$NAME=` and `$NAME` do not work there.

## Running

```sh
temoc -c Config.toml                      # everything in test_dir
temoc -c Config.toml specs/billing        # a directory (with recursive)
temoc -c Config.toml specs/late-fees.md   # one file
temoc -c Config.toml specs/late-fees.md:9 # the table whose directive is on line 9
temoc -c Config.toml specs/late-fees.md:13 # only the row on line 13
temoc -c Config.toml -f 'LateFee'         # tables whose fixture class matches a regex
temoc -c Config.toml -s                   # also list snoozed failures
temoc -c Config.toml -o                   # show the fixture server's stdout/stderr
temoc -c Config.toml --output jsonl       # one JSON event per line
```

`Config.toml`:

```toml
execute_server_command = "./target/debug/spec_fixtures %p"  # %p becomes the port
port = 1            # 1 = SLIM over stdin/stdout; anything else is TCP
pool_size = 1       # TCP only: number of ports to cycle through
test_dir = "specs/"
recursive = true
show_snoozed = true
pipe_output = false
persistent_server = true   # one server for the whole run
file_hooks = "SpecHooks"   # beginFile/endFile around every file
```

Build the fixture server before running Temoc and point
`execute_server_command` at the binary. Without `persistent_server` the
command runs once per file, so a command that compiles (`cargo run`,
`npm run`, ...) adds that cost to every file.

## Reading the result

Text output prints one line per file, then one line per failure with
`file:line:column` of the cell and the method called:

```
Testing file specs/late-fees.md...FAIL
Expected `4.00`, got `2.00` in specs/late-fees.md:13:44 for method call lateFee
Expected `1`, got Exception `NO_METHOD_IN_CLASS exp Calc.CalculatorFixture` in specs/calc.md:7:18 for method call exp
```

- `OK`: nothing failed. A snoozed table that passes also shows `OK`.
- `FAIL`: at least one real failure. Snoozed failures in the same file are
  listed too with `-s`, marked `-- snoozed until <date>`.
- `SNOOZED`: there are failures and all of them are snoozed.
- `NONE`: the file has no tables to run (after filters).

The exit code is the verdict: 0 when nothing really failed, 1 otherwise. An
expired snooze is a normal failure.

Telling failures apart while getting to red:

| Message | Meaning |
| ------- | ------- |
| ``Expected `X`, got `Y` `` | The behaviour differs. This is the red you want. |
| `NO_METHOD_IN_CLASS` | Header does not map to a fixture method (check the camelCase name) or the method is not written yet. |
| `NO_CLASS` | Fixture class not found or not registered; check `import`. Followed by `NO_INSTANCE` noise for the rest of the table. |
| Other `Exception` | The fixture or the system threw. |
| `Expected a test table...` / `Wrong number of columns...` | The markdown is malformed. Nothing ran. |

For tooling, or to get per-row status and actual values in a structured form,
use `--output jsonl`; the event format is described in the Temoc readme.

## Stopping early from a fixture

Fixtures can raise SLIM control exceptions: `ABORT_SLIM_TEST` (skip the rest
of the file), `ABORT_SLIM_SUITE` (stop the run), `IGNORE_SCRIPT_TEST` (skip
the rest of the table), `IGNORE_ALL_TESTS` (skip the rest of the file). Use
the aborts when a precondition for everything else is broken (database
unreachable), so the report shows one clear cause instead of hundreds of
follow-on failures.

## Fixture servers

Any SLIM server works; see <https://fitnesse.org/PlugIns.html>. For Rust, use
`rust_slim`:

```rust
use rust_slim::{fixture, PortSlimServer};

#[derive(Default)]
pub struct LateFeeFixture {
    amount: f64,
    days_overdue: i64,
}

#[fixture]
impl LateFeeFixture {
    pub fn set_amount(&mut self, amount: f64) {
        self.amount = amount;
    }

    pub fn set_days_overdue(&mut self, days: i64) {
        self.days_overdue = days;
    }

    pub fn late_fee(&self) -> String {
        format!("{:.2}", billing::late_fee(self.amount, self.days_overdue))
    }
}

fn main() -> anyhow::Result<()> {
    let mut server = PortSlimServer::listen_from_args(std::env::args().skip(1))?;
    server.add_fixture::<LateFeeFixture>();
    server.run()?;
    Ok(())
}
```

With port `1` the protocol owns stdout, so fixture code must not print to it.
