---
name: temoc-spec-driven-development
description: Spec-driven development with Temoc - write the behaviour as an executable markdown spec first, watch it fail, implement until it passes. Use when adding or changing a feature in a project that has Temoc specs (markdown files with `[//]: # (decisionTable ...)` directives, a Temoc config, or SLIM fixtures), when asked to write a spec, acceptance test or fixture for Temoc, or when a Temoc spec fails or is snoozed.
---

# Spec-driven development with Temoc

A Temoc spec is a markdown document that explains a behaviour in prose and
proves it with tables of examples. Temoc runs those tables against the real
system through fixtures. The spec is written **before** the code, and it is
the definition of done: the feature exists when the spec passes.

The spec is also the documentation people will read on GitHub a year from now.
Write it for that reader, and let the examples keep it honest.

Syntax, CLI flags and execution order are in [reference.md](reference.md).
Read it before writing your first table in a project.

## The loop

1. **Find where the behaviour belongs.** Read the existing specs. Extend the
   document that already describes this capability before creating a new one.
   A change to existing behaviour is an edit to existing rows, not a new file.
2. **Write the spec.** Prose that states the rule, then a table of examples.
   No production code yet. If the rule is unclear enough that you cannot fill
   in the expected column, that is a question for the user, not something to
   guess and encode.
3. **Add just enough fixture for the table to run**, with methods that
   delegate to code that does not exist or does not do the new thing yet.
4. **Run it and see it fail** (`temoc -c <config> path/to/spec.md:LINE`).
   Read the failure. It must fail *for the right reason*: `Expected X, got Y`
   on the new expectation, or a missing method for the new behaviour. A typo
   in a class name, a parse error or a crash in setup is not a red spec, it
   is a broken one. Fix it until the failure describes the missing feature.
   A spec that passes before you implement anything is testing nothing new:
   find out why.
5. **Snooze only if the failing spec has to be committed** while CI must stay
   green (spec merged ahead of the implementation, work split over several
   PRs). See [Snoozing](#snoozing). If spec and implementation land in the
   same change, skip the snooze.
6. **Implement** until the table passes. Drive the fine-grained design with
   unit tests; rerun the single table as the outer check.
7. **Remove the snooze**, run the whole suite, and confirm the file reports
   `OK`. Then tidy the spec and the fixtures while everything is green.

Never edit an expected value just to get to green. If the spec was wrong, say
so, and change it as a deliberate decision about behaviour.

## Snoozing

```markdown
[//]: # (decisionTable Billing.InvoiceFixture -- snooze until 2026-12-01)
```

A snooze turns the failures of one table into non-failures until the date
passes (inclusive). After that the table fails normally.

- Use a date days or a few weeks away, tied to when the work is expected to
  land. The expiry is the point: a forgotten spec comes back as a red build.
  Never use a far-future date as a way to switch a spec off.
- Snooze the smallest thing: the one table for the new behaviour. Put rows
  for the new behaviour in their own table so the rows that pass today stay
  enforced.
- Snooze is for behaviour that is not built yet. Never snooze a table that
  used to pass. That is a regression; fix it or raise it.
- Say in the surrounding prose that the behaviour is planned, so a reader
  does not take it for something that works.
- A snoozed table that passes prints `OK` with no warning. When the feature
  is done, remove the snooze in the same change. Search for `snooze until`
  before you call the work finished.
- A file is labelled `SNOOZED` only when every failure in it is snoozed; one
  real failure makes it `FAIL`. Run with `-s` to list the snoozed failures,
  marked `-- snoozed until <date>`.

## Writing the spec

**Prose and tables together.** The prose states the rule and why it exists;
the table shows it holding on concrete cases. A table with no prose is a test,
a paragraph with no table is a wish. Directives are invisible in rendered
markdown, so the page must read well as a plain document.

**One rule per table.** If a table needs "and also" to describe, split it.
Small tables (roughly four to ten rows) with a sentence in front of each beat
one wide table that covers everything.

**Speak the domain's language.** Column headers and cell values use the words
the users and the business use: `customer tier`, `order total`, `discount?`.
No ids, SQL, URLs, JSON, CSS selectors or class names in a table. If a header
only makes sense to someone who has read the code, rename it.

**Say what, not how.** Describe the outcome, not the steps to get there.
`| logged in as | can refund? |` rather than a sequence of "open page, type,
click". The how lives in the fixture, where it can change without touching
the spec.

**Given / When / Then maps onto columns.** Input columns are the context and
the action; `?` columns are the outcomes. Keep them in that order, left to
right.

**Pick examples that teach.** One plain case, then one row per boundary and
per rule interaction: just below, on, and just above a threshold; empty; the
maximum. Each row should exist for a reason that a `# why` comment column can
state in a few words. Exhaustive combinations belong in unit tests.

**Only essential columns.** A column that has the same value in every row is
noise: give it a default in the fixture. A reader should be able to see which
input caused which output.

**Concrete values.** `150.00`, not "a large order". Real-looking names and
amounts make wrong expectations easy to spot in review.

**Specify the refusals too.** Invalid input, missing permission and
conflicting state are behaviour. Give them rows, with the outcome as a value
(`rejected because?` = `insufficient funds`), not as an exception.

**Stateful flows: build, operate, check.** Temoc has decision tables only, so
describe a flow as consecutive tables with prose between them: one that sets
up the given state, one that performs the action, one that checks what is
true afterwards. Pass generated values between them with symbols (`$ID=`,
then `$ID`).

**Keep each file independent.** Symbols do not cross files, and by default
every file runs against a fresh server process. A spec must never depend on another
file having run first, or on file order. Within a file, prefer rows that do
not depend on earlier rows: a row selected with `file.md:LINE` runs alone.

**Make it deterministic.** Same result every run, on every machine. Control
the clock, random numbers and generated ids from the fixture. Do not put
today's date or a generated id in an expected cell.

**Organise by capability, not by history.** Files and headings are named for
what the system does (`invoicing/late-fees.md`), not for tickets, sprints or
who asked. Do not leave change logs in specs; git has that.

**Stay at the level of observable behaviour.** A spec states what someone
outside the system can rely on. It should survive a rewrite of the internals.
If a refactor with no behaviour change breaks a spec, the spec was coupled to
the implementation.

## Writing fixtures

A fixture is glue: it turns cells into calls on the system and results into
cells. Nothing else.

**Thin.** Parse the cell, delegate to application code, format the result.
No business logic, no branching on the scenario, no computing the expected
answer. If a fixture is getting clever, the logic belongs in the application,
or the table is asking the wrong question.

**Let the table do the asserting.** Return the actual value and let Temoc
compare it. A fixture that asserts internally and returns `true` hides what
was actually observed.

**Enter the system below the UI by default.** Call the same service, use-case
or handler layer the UI calls. It is faster, more stable and tests the rule
rather than the widget. Go through the UI only for specs about UI behaviour.

**Use the real things you own; fake the things you don't.**
- Database: use the real one. Queries, constraints, transactions and
  migrations are where much real behaviour lives, and a mocked repository
  proves little.
- Third-party services (payment providers, email, external APIs): replace at
  the boundary your code owns, with an in-process fake that records calls and
  returns scripted answers. The spec then states your behaviour given their
  answer. A spec must pass with the network unplugged.
- Do not start services the behaviour does not need.

**Fast.** A slow suite stops being run, and then it stops being true.
- No sleeps. Ever. For time-dependent rules, inject a clock and move it from
  the fixture (`| days since due | late fee? |`). For asynchronous work, run
  it inline in tests (call the job handler, drain the queue synchronously).
  Where that is truly impossible, wait on the condition itself with a short
  bounded deadline, never a fixed delay.
- By default Temoc starts one server process per spec file. Point
  `execute_server_command` at a prebuilt binary instead of a command that
  compiles, and run migrations once against a prepared database rather than
  on every start. If startup is still expensive (booting an application),
  set `persistent_server = true` so the server starts once per run.
- With a persistent server nothing is reset between files, so isolation is
  the fixtures' job: configure a `file_hooks` fixture and restore a known
  state in `beginFile` / `endFile`.
- Isolate database state cheaply: wrap each file (file hooks) or each row
  (`reset`) in a transaction that is rolled back, or delete only what was
  created. Do not recreate the schema per table. Use `beginTable` /
  `endTable` for per-table setup and teardown.
- Create only the data the row needs, through the application's own code
  paths where practical so the data is valid by construction.

**Leave nothing behind.** Each row and each file starts from a known state
and cleans up after itself, so specs can run in any order and repeatedly.

**Stable, readable output.** Comparison is exact text. Format in the fixture
on purpose (`12.50`, `2026-03-01`, `yes`/`no`), in the form the spec reader
expects, and keep it consistent across fixtures.

**Expected refusals are values; surprises are exceptions.** Return
`insufficient funds` for a rule the spec describes. Let genuine faults throw,
so they show up as errors instead of as a mismatched cell.

**Name fixtures for the domain concept** (`LateFeeFixture`), and reuse one
across tables and files. Fixture code is production-grade code: review it,
refactor it, remove duplication into shared helpers.

## GUI, TUI and web: behaviour, not style

When the behaviour under specification is the interface itself, specify what
the user can do and what they come to know, not how it looks.

Specify:
- what information is shown (the text and values a user reads);
- what actions are available, unavailable or disabled, and for whom;
- what happens after an action: the resulting state, message, navigation;
- validation and error feedback; focus and keyboard flow where it matters.

Do not specify colours, fonts, sizes, spacing, positions, pixel comparisons,
CSS classes, DOM structure, ANSI escape codes or animation. Those change for
reasons that have nothing to do with behaviour, and every such change would
break a spec that was still true.

In the fixture:
- Find elements the way a user does: by role, label or visible text, falling
  back to a dedicated test id. Never by styling class or DOM path.
- Hide the mechanics behind a small driver (page/screen object) so tables
  stay in domain words: `| signed in as | sees refund button? |`.
- Web: run headless, against the app started in-process or locally, with the
  same faked third parties. Wait for a condition (element present, request
  settled), never for time.
- TUI: drive the application's update loop or a virtual terminal directly,
  send key events, and read back text content. Assert on what a region says,
  not on escape sequences or exact cell coordinates.
- Keep these specs few. A rule tested through the UI should be one that only
  exists in the UI; everything else is specified below it, once.

## Before you call it done

- [ ] The spec was written first and seen failing for the right reason.
- [ ] Every table has prose saying what rule it demonstrates.
- [ ] Headers and values are domain words; no implementation detail leaks.
- [ ] Boundaries and refusals have rows; no row is there without a reason.
- [ ] Fixtures only translate and delegate.
- [ ] No sleeps, no network to third parties, no dependence on file order.
- [ ] The file passes alone, twice in a row, and the whole suite passes.
- [ ] No `snooze until` is left on anything you implemented.
- [ ] The rendered markdown reads as documentation to someone who has never
      seen the code.

## Smells

| Smell | What to do |
| ----- | ---------- |
| Spec written after the code, shaped to match it | Write the next one first; here, check the rows against the intended rule, not the output |
| Passes on first run | Find out what it is not exercising |
| Table reads like a script of clicks or API calls | Raise it to the outcome; move the steps into the fixture |
| Wide table, many columns identical in every row | Split by rule; default the constant inputs |
| Expected value copied from what the code returned | Derive it from the rule by hand |
| Fixture with `if` on scenario names, or computing results | Move logic to the application or split the fixture |
| Mocked database | Use the real one with cheap isolation |
| `sleep`, retries with fixed delays | Inject the clock; run async work inline; wait on a condition |
| Spec fails only when run after another file | Shared state is leaking; clean up per row/file |
| Snooze dated years ahead, or on a table that used to pass | Give it a real date, or fix the regression |
| Asserting on colours, classes or layout | Assert on what the user can read and do |
| Same rule checked in five specs | Specify it once, at the lowest level that shows it |
