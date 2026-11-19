//! Runs the real binary against two real Slim servers, to check that every
//! table runs on the server its page names.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const TEMOC: &str = env!("CARGO_BIN_EXE_temoc");
static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

/// A command that starts the probe server under a name.
fn probe(name: &str) -> String {
    let probe = Path::new(TEMOC)
        .parent()
        .unwrap()
        .join("examples/jsonl_probe");
    format!("PROBE_NAME={name} {} %p", probe.display())
}

fn workspace() -> PathBuf {
    let id = NEXT_DIR.fetch_add(1, Ordering::SeqCst);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("servers-{id}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, name: &str, content: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path.display().to_string()
}

fn server_table(name: &str, command: &str, settings: &str) -> String {
    format!("[servers.{name}]\nexecute_server_command = \"{command}\"\nport = 1\n{settings}\n")
}

/// A configuration with the servers `server`, which is the default, and
/// `machines`.
fn config(dir: &Path, server: &str, machines: &str) -> String {
    let content =
        server_table("server", server, "default = true") + &server_table("machines", machines, "");
    write(dir, "temoc.toml", &content)
}

fn both(dir: &Path) -> String {
    config(dir, &probe("server"), &probe("machines"))
}

fn directive(text: &str) -> String {
    format!("[//]: # ({text})\n")
}

fn table(class: &str, rows: &str) -> String {
    format!(
        "{}\n{rows}\n\n",
        directive(&format!("decisionTable {class}"))
    )
}

/// A table that passes only on the server with that name.
fn who(name: &str) -> String {
    table("WhoAmI", &format!("| name? |\n|---|\n| {name} |"))
}

fn on(server: &str) -> String {
    directive(&format!("server {server}"))
}

struct Text {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

fn text(args: &[&str]) -> Text {
    let output = Command::new(TEMOC).args(args).output().unwrap();
    Text {
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
        code: output.status.code(),
    }
}

fn events(output: &Output) -> Vec<Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|_| panic!("not JSON: {line}")))
        .collect()
}

fn jsonl(args: &[&str]) -> (Vec<Value>, Option<i32>) {
    let output = Command::new(TEMOC)
        .args(["--output", "jsonl"])
        .args(args)
        .output()
        .unwrap();
    let events = events(&output);
    assert_eq!("run_finished", events.last().unwrap()["type"]);
    (events, output.status.code())
}

fn of_type<'a>(events: &'a [Value], kind: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| event["type"] == kind)
        .collect()
}

/// The server and the status of every row, in order.
fn rows(events: &[Value]) -> Vec<(String, String)> {
    of_type(events, "row_finished")
        .iter()
        .map(|row| {
            (
                row["server"].as_str().unwrap().to_string(),
                row["status"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn pairs(rows: &[(&str, &str)]) -> Vec<(String, String)> {
    rows.iter()
        .map(|(server, status)| ((*server).to_string(), (*status).to_string()))
        .collect()
}

#[test]
fn a_single_server_configuration_runs_as_before() {
    let dir = workspace();
    let config = write(
        &dir,
        "temoc.toml",
        &format!("execute_server_command = \"{}\"\nport = 1\n", probe("only")),
    );
    let passing = write(&dir, "a.md", &who("only"));
    let failing = write(&dir, "b.md", &who("other"));
    let run = text(&["-c", &config, &passing, &failing]);
    assert_eq!(
        format!(
            "Testing file {passing}...OK\nTesting file {failing}...FAIL\nExpected `other`, got `only` in {failing}:5:3 for method call name\n"
        ),
        run.stdout
    );
    assert_eq!(Some(1), run.code);

    let (events, code) = jsonl(&["-c", &config, &passing]);
    assert_eq!(Some(0), code);
    assert_eq!(pairs(&[("default", "passed")]), rows(&events));
}

#[test]
fn a_page_without_a_server_directive_runs_on_the_default() {
    let dir = workspace();
    // The other server would fail, if it were started.
    let config = config(&dir, &probe("server"), "false");
    let file = write(&dir, "a.md", &(who("server") + &who("server")));
    let run = text(&["-c", &config, &file]);
    assert_eq!(format!("Testing file {file}...OK\n"), run.stdout);
    assert_eq!(Some(0), run.code);

    let (events, code) = jsonl(&["-c", &config, &file]);
    assert_eq!(Some(0), code);
    assert_eq!(
        pairs(&[("server", "passed"), ("server", "passed")]),
        rows(&events)
    );
    assert!(of_type(&events, "run_error").is_empty());
}

#[test]
fn a_server_directive_at_the_top_puts_every_table_on_that_server() {
    let dir = workspace();
    // The default server would fail, if it were started.
    let config = config(&dir, "false", &probe("machines"));
    let content =
        on("machines") + &who("machines") + &table("MachinesOnly", "| here? |\n|---|\n| yes |");
    let file = write(&dir, "a.md", &content);
    let run = text(&["-c", &config, &file]);
    assert_eq!(format!("Testing file {file}...OK\n"), run.stdout);
    assert_eq!(Some(0), run.code);

    let (events, code) = jsonl(&["-c", &config, &file]);
    assert_eq!(Some(0), code);
    assert_eq!(
        pairs(&[("machines", "passed"), ("machines", "passed")]),
        rows(&events)
    );
    let tables = &of_type(&events, "file_started")[0]["tables"];
    assert_eq!("machines", tables[0]["server"]);
    assert_eq!("machines", of_type(&events, "table_started")[0]["server"]);
    assert_eq!("machines", of_type(&events, "table_finished")[0]["server"]);
    assert_eq!("machines", of_type(&events, "row_started")[0]["server"]);
}

#[test]
fn a_page_can_switch_servers_between_tables() {
    let dir = workspace();
    let config = both(&dir);
    // Both spellings of a directive select a server.
    let content = who("server")
        + &on("machines")
        + &who("machines")
        + &table("MachinesOnly", "| here? |\n|---|\n| yes |")
        + "[//]: # \"server server\"\n\n"
        + &who("server");
    let file = write(&dir, "a.md", &content);
    let run = text(&["-c", &config, &file]);
    assert_eq!(format!("Testing file {file}...OK\n"), run.stdout);
    assert_eq!(Some(0), run.code);

    let (events, code) = jsonl(&["-c", &config, &file]);
    assert_eq!(Some(0), code);
    assert_eq!(
        pairs(&[
            ("server", "passed"),
            ("machines", "passed"),
            ("machines", "passed"),
            ("server", "passed")
        ]),
        rows(&events)
    );
}

#[test]
fn a_fixture_of_the_other_server_is_not_found() {
    let dir = workspace();
    let config = both(&dir);
    let content = table("MachinesOnly", "| here? |\n|---|\n| yes |") + &on("machines") + &who("x");
    let file = write(&dir, "a.md", &content);
    let run = text(&["-c", &config, &file]);
    assert_eq!(Some(1), run.code);
    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(format!("Testing file {file}...FAIL"), lines[0]);
    assert!(
        lines[1].starts_with("Expected OK, got Exception"),
        "{run:?}"
    );
    assert!(lines[1].ends_with(&format!("in {file}:1:1 on server server")));
    // A file with several servers says which one a failure is from.
    assert_eq!(
        format!(
            "Expected `x`, got `machines` in {file}:12:3 for method call name on server machines"
        ),
        *lines.last().unwrap()
    );
}

impl std::fmt::Debug for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stdout: {}\nstderr: {}", self.stdout, self.stderr)
    }
}

#[test]
fn an_unknown_server_fails_the_file_at_its_line_and_runs_no_table() {
    let dir = workspace();
    let config = both(&dir);
    let content = who("server") + &on("nosuch") + &who("server");
    let file = write(&dir, "a.md", &content);
    let run = text(&["-c", &config, &file]);
    assert_eq!(Some(1), run.code);
    assert_eq!(format!("Testing file {file}..."), run.stdout);
    assert!(
        run.stderr.contains(&format!(
            "Unknown server `nosuch` at {file}:7:1. The configured servers are: `machines`, `server`"
        )),
        "{run:?}"
    );

    let next = write(&dir, "b.md", &who("server"));
    let (events, code) = jsonl(&["-c", &config, &file, &next]);
    assert_eq!(Some(1), code);
    let error = of_type(&events, "run_error")[0];
    assert_eq!("parse_error", error["kind"]);
    assert_eq!(file, error["file"]);
    assert!(error["message"]
        .as_str()
        .unwrap()
        .starts_with(&format!("Unknown server `nosuch` at {file}:7:1")));
    assert_eq!(pairs(&[("server", "passed")]), rows(&events));
    assert_eq!(1, of_type(&events, "file_started").len());
}

#[test]
fn several_servers_without_a_default_are_refused() {
    let dir = workspace();
    let content = server_table("server", &probe("server"), "")
        + &server_table("machines", &probe("machines"), "");
    let config = write(&dir, "temoc.toml", &content);
    let file = write(&dir, "a.md", &who("server"));
    let run = text(&["-c", &config, &file]);
    assert_eq!(Some(1), run.code);
    assert_eq!("", run.stdout);
    assert!(
        run.stderr.contains(
            "Several servers are configured (`machines`, `server`) and none is the default. Add `default = true` to the one that pages without a `server` directive use"
        ),
        "{run:?}"
    );

    let (events, code) = jsonl(&["-c", &config, &file]);
    assert_eq!(Some(1), code);
    assert_eq!("bad_config", of_type(&events, "run_error")[0]["kind"]);
    assert!(of_type(&events, "file_started").is_empty());
}

#[test]
fn server_settings_at_the_top_level_can_not_be_mixed_with_named_servers() {
    let dir = workspace();
    let content = "port = 1\n".to_string() + &server_table("server", &probe("server"), "");
    let config = write(&dir, "temoc.toml", &content);
    let file = write(&dir, "a.md", &who("server"));
    let run = text(&["-c", &config, &file]);
    assert_eq!(Some(1), run.code);
    assert!(
        run.stderr.contains(
            "`port` is set at the top level of a configuration that also has `[servers.*]`"
        ),
        "{run:?}"
    );
}

#[test]
fn a_server_that_no_page_uses_is_never_started() {
    let dir = workspace();
    let marker = dir.join("started");
    let machines = format!("touch {}; false", marker.display());
    let config = config(&dir, &probe("server"), &machines);
    let a = write(&dir, "a.md", &who("server"));
    // An import alone doesn't start a server either.
    let b = write(
        &dir,
        "b.md",
        &(who("server") + &on("machines") + &directive("import Fixtures")),
    );
    let run = text(&["-c", &config, &a, &b]);
    assert_eq!(
        format!("Testing file {a}...OK\nTesting file {b}...OK\n"),
        run.stdout
    );
    assert_eq!(Some(0), run.code);

    let (events, code) = jsonl(&["-c", &config, &a, &b]);
    assert_eq!(Some(0), code);
    assert_eq!(
        pairs(&[("server", "passed"), ("server", "passed")]),
        rows(&events)
    );
    assert!(!marker.exists(), "the second server was started");
}

#[test]
fn selecting_a_server_skips_the_tables_of_the_others() {
    let dir = workspace();
    let marker = dir.join("started");
    let server = format!("touch {}; false", marker.display());
    let config = config(&dir, &server, &probe("machines"));
    let mixed = write(
        &dir,
        "a.md",
        &(who("server") + &on("machines") + &who("machines")),
    );
    let default_only = write(&dir, "b.md", &who("server"));
    let run = text(&["-c", &config, "--server", "machines", &mixed, &default_only]);
    assert_eq!(
        format!("Testing file {mixed}...OK\nTesting file {default_only}...NONE\n"),
        run.stdout,
        "{run:?}"
    );
    assert_eq!(Some(0), run.code);

    let (events, code) = jsonl(&["-c", &config, "--server", "machines", &mixed, &default_only]);
    assert_eq!(Some(0), code);
    assert_eq!(
        pairs(&[
            ("server", "skipped"),
            ("machines", "passed"),
            ("server", "skipped")
        ]),
        rows(&events)
    );
    assert!(of_type(&events, "run_error").is_empty());
    assert!(!marker.exists(), "the server that was left out was started");
}

#[test]
fn several_servers_can_be_selected() {
    let dir = workspace();
    let config = both(&dir);
    let file = write(
        &dir,
        "a.md",
        &(who("server") + &on("machines") + &who("machines")),
    );
    for selection in [
        vec!["--server", "machines,server"],
        vec!["--server", "machines", "--server", "server"],
    ] {
        let mut args = vec!["-c", config.as_str()];
        args.extend(selection);
        args.push(&file);
        let (events, code) = jsonl(&args);
        assert_eq!(Some(0), code);
        assert_eq!(
            pairs(&[("server", "passed"), ("machines", "passed")]),
            rows(&events)
        );
    }
}

#[test]
fn selecting_an_unknown_server_is_an_error_before_anything_runs() {
    let dir = workspace();
    let config = both(&dir);
    let file = write(&dir, "a.md", &who("server"));
    let run = text(&["-c", &config, "--server", "nosuch", &file]);
    assert_eq!(Some(1), run.code);
    assert_eq!("", run.stdout);
    assert!(
        run.stderr.contains(
            "Unknown server `nosuch` in --server. The configured servers are: `machines`, `server`"
        ),
        "{run:?}"
    );

    let (events, code) = jsonl(&["-c", &config, "--server", "nosuch", &file]);
    assert_eq!(Some(1), code);
    assert_eq!("bad_config", of_type(&events, "run_error")[0]["kind"]);
}

#[test]
fn a_server_selection_combines_with_the_class_filter() {
    let dir = workspace();
    let config = config(&dir, "false", &probe("machines"));
    let content =
        on("machines") + &who("wrong") + &table("MachinesOnly", "| here? |\n|---|\n| yes |");
    let file = write(&dir, "a.md", &content);
    let run = text(&[
        "-c",
        &config,
        "--server",
        "machines",
        "-f",
        "MachinesOnly",
        &file,
    ]);
    assert_eq!(format!("Testing file {file}...OK\n"), run.stdout);
}

const HOOK_LOG: &str = "| hook log? |\n|---|\n";

fn hook_log(expected: &str) -> String {
    table("Probe", &format!("{HOOK_LOG}| {expected} |"))
}

#[test]
fn file_hooks_run_once_for_each_file_on_the_servers_it_uses() {
    let dir = workspace();
    let settings = "persistent_server = true\nfile_hooks = \"Hooks\"";
    let content = server_table(
        "server",
        &probe("server"),
        &format!("default = true\n{settings}"),
    ) + &server_table("machines", &probe("machines"), settings);
    let config = write(&dir, "temoc.toml", &content);
    let a = write(&dir, "a.md", &hook_log("begin a.md"));
    // Two tables on each server, and still one `beginFile` on each.
    let b = write(
        &dir,
        "b.md",
        &(hook_log("begin a.md,end,begin b.md")
            + &on("machines")
            + &hook_log("begin b.md")
            + &on("server")
            + &hook_log("begin a.md,end,begin b.md")
            + &on("machines")
            + &hook_log("begin b.md")),
    );
    let c = write(
        &dir,
        "c.md",
        &(on("machines") + &hook_log("begin b.md,end,begin c.md")),
    );
    // The default server never saw `c.md`.
    let d = write(
        &dir,
        "d.md",
        &hook_log("begin a.md,end,begin b.md,end,begin d.md"),
    );
    let run = text(&["-c", &config, &a, &b, &c, &d]);
    assert_eq!(
        format!(
            "Testing file {a}...OK\nTesting file {b}...OK\nTesting file {c}...OK\nTesting file {d}...OK\n"
        ),
        run.stdout,
        "{run:?}"
    );
    assert_eq!(Some(0), run.code);

    let (events, code) = jsonl(&["-c", &config, &a, &b, &c, &d]);
    assert_eq!(Some(0), code, "{events:#?}");
    assert!(of_type(&events, "setup_failed").is_empty());
    assert_eq!(
        vec!["passed"; 7],
        rows(&events)
            .into_iter()
            .map(|(_, status)| status)
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_failed_begin_file_only_skips_the_tables_of_its_server() {
    let dir = workspace();
    let content = server_table("server", &probe("server"), "default = true")
        + &server_table("machines", &probe("machines"), "file_hooks = \"Hooks\"");
    let config = write(&dir, "temoc.toml", &content);
    // The hooks of the probe fail for a file with this name.
    let file = write(
        &dir,
        "broken.md",
        &(who("server") + &on("machines") + &who("wrong") + &on("server") + &who("server")),
    );
    let run = text(&["-c", &config, &file]);
    assert_eq!(
        format!(
            "Testing file {file}...FAIL\nExpected NULL or VOID or NOT FOUND, got Exception `bad begin` in {file}:0:0 for method call beginFile on server machines\n"
        ),
        run.stdout
    );
    assert_eq!(Some(1), run.code);

    let config = write(&dir, "piped.toml", &(content + "pipe_output = true\n"));
    let (events, code) = jsonl(&["-c", &config, &file]);
    assert_eq!(Some(1), code);
    assert_eq!(
        pairs(&[
            ("server", "passed"),
            ("machines", "skipped"),
            ("server", "passed")
        ]),
        rows(&events)
    );
    let failures = of_type(&events, "setup_failed");
    assert_eq!(1, failures.len());
    assert_eq!("begin_file", failures[0]["phase"]);
    assert_eq!("machines", failures[0]["server"]);
    // `endFile` still runs, on the server that began the file.
    let output = of_type(&events, "output");
    assert_eq!(1, output.len());
    assert_eq!("end_file ran\n", output[0]["text"]);
    assert_eq!("machines", output[0]["server"]);
}

const VALUES: &str = "| value | value? |\n|---|---|\n";

#[test]
fn a_symbol_of_one_server_can_not_be_used_on_the_other() {
    let dir = workspace();
    let config = both(&dir);
    let content = table("Probe", &format!("{VALUES}| 7 | $X= |\n| $X | $X |"))
        + &on("machines")
        + &table("Probe", &format!("{VALUES}| 7 | $X |\n| $X | 7 |"));
    let file = write(&dir, "a.md", &content);
    let run = text(&["-c", &config, &file]);
    assert_eq!(
        format!(
            "Testing file {file}...FAIL\n\
             Symbol `$X` was set on server `server` and cannot be used on server `machines` in {file}:14:3\n\
             Symbol `$X` was set on server `server` and cannot be used on server `machines` in {file}:13:7\n\
             Expected `7`, got `$X` in {file}:14:8 for method call value on server machines\n"
        ),
        run.stdout,
        "{run:?}"
    );
    assert_eq!(Some(1), run.code);

    let (events, code) = jsonl(&["-c", &config, &file]);
    assert_eq!(Some(1), code);
    assert_eq!(
        pairs(&[
            ("server", "passed"),
            ("server", "passed"),
            ("machines", "error"),
            ("machines", "error")
        ]),
        rows(&events)
    );
    let rows = of_type(&events, "row_finished");
    assert_eq!(
        json!({
            "kind": "symbol_from_other_server",
            "symbol": "X",
            "server": "server",
            "message": format!("Symbol `$X` was set on server `server` and cannot be used on server `machines` in {file}:13:7"),
        }),
        rows[2]["checks"][1]["actual"]
    );
    // A row with such an input cell is not sent.
    assert_eq!(
        "symbol_from_other_server",
        rows[3]["checks"][0]["actual"]["kind"]
    );
    assert_eq!("skipped", rows[3]["checks"][1]["status"]);
}

#[test]
fn each_server_can_set_its_own_symbol_of_the_same_name() {
    let dir = workspace();
    let config = both(&dir);
    let content = table("Probe", &format!("{VALUES}| 7 | $X= |"))
        + &on("machines")
        + &table("Probe", &format!("{VALUES}| 8 | $X= |\n| $X | 8 |"))
        + &on("server")
        + &table("Probe", &format!("{VALUES}| $X | 7 |"));
    let file = write(&dir, "a.md", &content);
    let run = text(&["-c", &config, &file]);
    assert_eq!(format!("Testing file {file}...OK\n"), run.stdout, "{run:?}");

    let (events, code) = jsonl(&["-c", &config, &file]);
    assert_eq!(Some(0), code, "{events:#?}");
}

#[test]
fn a_snoozed_table_on_another_server_is_snoozed() {
    let dir = workspace();
    let config = both(&dir);
    let content = on("machines")
        + &table(
            "WhoAmI -- snooze until 2099-12-31",
            "| name? |\n|---|\n| wrong |",
        )
        + &table(
            "NoSuchClass -- snooze until 2099-12-31",
            "| name? |\n|---|\n| x |",
        );
    let file = write(&dir, "a.md", &content);
    let run = text(&["-c", &config, &file]);
    assert_eq!(format!("Testing file {file}...SNOOZED\n"), run.stdout);
    assert_eq!(Some(0), run.code);

    let (events, code) = jsonl(&["-c", &config, &file]);
    assert_eq!(Some(0), code);
    assert_eq!("snoozed", events.last().unwrap()["status"]);
}

#[test]
fn a_server_that_does_not_start_fails_its_tables_and_the_others_still_run() {
    let dir = workspace();
    let config = config(&dir, &probe("server"), "echo no toolchain >&2; exit 7");
    let mixed = write(
        &dir,
        "a.md",
        &(who("server")
            + &on("machines")
            + &who("machines")
            + &who("machines")
            + &on("server")
            + &who("server")),
    );
    let default_only = write(&dir, "b.md", &who("server"));
    let machines_only = write(&dir, "c.md", &(on("machines") + &who("machines")));
    let run = text(&["-c", &config, &mixed, &default_only, &machines_only]);
    let failure = "Server `machines` (`echo no toolchain >&2; exit 7`) failed to start: ";
    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(5, lines.len(), "{run:?}");
    assert_eq!(format!("Testing file {mixed}...FAIL"), lines[0]);
    assert!(lines[1].starts_with(failure), "{run:?}");
    assert_eq!(format!("Testing file {default_only}...OK"), lines[2]);
    assert_eq!(format!("Testing file {machines_only}...FAIL"), lines[3]);
    assert!(lines[4].starts_with(failure), "{run:?}");
    assert_eq!(Some(1), run.code);

    let (events, code) = jsonl(&["-c", &config, &mixed, &default_only, &machines_only]);
    assert_eq!(Some(1), code);
    assert_eq!(
        pairs(&[
            ("server", "passed"),
            ("machines", "error"),
            ("machines", "error"),
            ("server", "passed"),
            ("server", "passed"),
            ("machines", "error"),
        ]),
        rows(&events)
    );
    // The run goes on, and the server is reported once.
    assert_eq!(3, of_type(&events, "file_started").len());
    let errors = of_type(&events, "run_error");
    assert_eq!(1, errors.len());
    assert_eq!("server_start_failed", errors[0]["kind"]);
    assert_eq!("machines", errors[0]["server"]);
    assert_eq!("no toolchain\n", errors[0]["stderr"]);
    assert!(errors[0]["message"].as_str().unwrap().starts_with(failure));
    let row = of_type(&events, "row_finished")[1];
    assert!(row["message"].as_str().unwrap().starts_with(failure));
}

#[test]
fn a_server_that_dies_fails_its_tables_and_the_others_still_run() {
    let dir = workspace();
    let config = both(&dir);
    let content = who("server")
        + &on("machines")
        + &table("Probe", "| die? |\n|---|\n| x |\n| x |")
        + &who("machines")
        + &on("server")
        + &who("server");
    let file = write(&dir, "a.md", &content);
    let next = write(&dir, "b.md", &who("server"));
    let run = text(&["-c", &config, &file, &next]);
    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(3, lines.len(), "{run:?}");
    assert_eq!(format!("Testing file {file}...FAIL"), lines[0]);
    assert!(
        lines[1].starts_with(&format!(
            "Server `machines` (`{}`) was lost: ",
            probe("machines")
        )),
        "{run:?}"
    );
    assert_eq!(format!("Testing file {next}...OK"), lines[2]);
    assert_eq!(Some(1), run.code);

    let (events, code) = jsonl(&["-c", &config, &file, &next]);
    assert_eq!(Some(1), code);
    assert_eq!(
        pairs(&[
            ("server", "passed"),
            ("machines", "error"),
            ("machines", "skipped"),
            ("machines", "error"),
            ("server", "passed"),
            ("server", "passed"),
        ]),
        rows(&events)
    );
    let errors = of_type(&events, "run_error");
    assert_eq!(1, errors.len());
    assert_eq!("server_died", errors[0]["kind"]);
    assert_eq!("machines", errors[0]["server"]);
}

#[test]
fn the_only_server_failing_names_it_and_stops_the_run() {
    let dir = workspace();
    let file = write(&dir, "a.md", &who("x"));
    let next = write(&dir, "b.md", &who("x"));
    let run = text(&["-p", "1", "-x", "exit 7", &file, &next]);
    assert_eq!(Some(1), run.code);
    assert_eq!(format!("Testing file {file}..."), run.stdout);
    assert!(
        run.stderr
            .contains("Server `default` (`exit 7`) failed to start: "),
        "{run:?}"
    );
}

#[test]
fn a_line_selects_a_table_or_a_row_on_the_server_named_above_it() {
    let dir = workspace();
    // The default server would fail, if the selected table ran on it.
    let config = config(&dir, "false", &probe("machines"));
    let content = who("server")
        + &on("machines")
        + &table("WhoAmI", "| name? |\n|---|\n| wrong |\n| machines |");
    let file = write(&dir, "a.md", &content);
    let row = format!("{file}:13");
    let run = text(&["-c", &config, &row]);
    assert_eq!(format!("Testing file {file}...OK\n"), run.stdout, "{run:?}");
    assert_eq!(Some(0), run.code);

    let (events, code) = jsonl(&["-c", &config, &row]);
    assert_eq!(Some(0), code);
    assert_eq!(pairs(&[("machines", "passed")]), rows(&events));

    let table = format!("{file}:8");
    let run = text(&["-c", &config, &table]);
    assert_eq!(
        format!(
            "Testing file {file}...FAIL\nExpected `wrong`, got `machines` in {file}:12:3 for method call name\n"
        ),
        run.stdout,
        "{run:?}"
    );
    let (events, _) = jsonl(&["-c", &config, &table]);
    assert_eq!(
        pairs(&[("machines", "failed"), ("machines", "passed")]),
        rows(&events)
    );
}

#[test]
fn the_command_line_sets_the_default_server() {
    let dir = workspace();
    let config = config(&dir, "false", &probe("machines"));
    let file = write(
        &dir,
        "a.md",
        &(who("other") + &on("machines") + &who("machines")),
    );
    let run = text(&["-c", &config, "-x", &probe("other"), &file]);
    assert_eq!(format!("Testing file {file}...OK\n"), run.stdout, "{run:?}");
}

#[test]
fn piped_output_says_which_server_it_is_from() {
    let dir = workspace();
    let content = server_table(
        "server",
        &probe("server"),
        "default = true\npipe_output = true",
    ) + &server_table("machines", &probe("machines"), "pipe_output = true");
    let config = write(&dir, "temoc.toml", &content);
    let noisy = table("Probe", "| noisy? |\n|---|\n| done |");
    let file = write(&dir, "a.md", &(noisy.clone() + &on("machines") + &noisy));
    let run = text(&["-c", &config, &file]);
    assert_eq!(Some(0), run.code, "{run:?}");
    assert!(run.stdout.contains("[server] to stderr\n"), "{run:?}");
    assert!(run.stdout.contains("[machines] to stderr\n"), "{run:?}");

    let (events, _) = jsonl(&["-c", &config, &file]);
    let servers: Vec<&Value> = of_type(&events, "output")
        .iter()
        .map(|output| &output["server"])
        .collect();
    assert_eq!(vec![&json!("server"), &json!("machines")], servers);

    // With one server, the lines are as the server wrote them.
    let alone = write(&dir, "b.md", &noisy);
    let run = text(&["-o", "-p", "1", "-x", &probe("only"), &alone]);
    assert_eq!(
        format!("Testing file {alone}...to stderr\nOK\n"),
        run.stdout
    );
}

#[test]
fn a_control_exception_stops_the_file_on_every_server() {
    let dir = workspace();
    let config = both(&dir);
    let content = on("machines")
        + &table("Probe", "| abort test? |\n|---|\n| x |")
        + &who("machines")
        + &on("server")
        + &who("server");
    let file = write(&dir, "a.md", &content);
    let next = write(&dir, "b.md", &who("server"));
    let (events, code) = jsonl(&["-c", &config, &file, &next]);
    assert_eq!(Some(1), code);
    assert_eq!(
        pairs(&[
            ("machines", "error"),
            ("machines", "skipped"),
            ("server", "skipped"),
            ("server", "passed"),
        ]),
        rows(&events)
    );
}
