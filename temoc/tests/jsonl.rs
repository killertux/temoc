//! Runs the real binary against a real Slim server, for `--output jsonl`.

use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const TEMOC: &str = env!("CARGO_BIN_EXE_temoc");
static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

fn probe() -> String {
    Path::new(TEMOC)
        .parent()
        .unwrap()
        .join("examples/jsonl_probe")
        .display()
        .to_string()
}

fn workspace() -> PathBuf {
    let id = NEXT_DIR.fetch_add(1, Ordering::SeqCst);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("jsonl-{id}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, name: &str, content: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path.display().to_string()
}

fn table(directive: &str, rows: &str) -> String {
    format!("[//]: # \"decisionTable {directive}\"\n\n{rows}\n\n")
}

fn temoc(server: &str, args: &[&str]) -> Command {
    let mut command = Command::new(TEMOC);
    command
        .args(["--output", "jsonl", "-x", server, "-p", "1"])
        .args(args);
    command
}

/// Every stdout line must parse, in every scenario.
fn events(output: &Output) -> Vec<Value> {
    let events: Vec<Value> = String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|_| panic!("not JSON: {line}")))
        .collect();
    assert_eq!("run_started", events[0]["type"]);
    assert_eq!(
        "run_finished",
        events.last().unwrap()["type"],
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_rows_are_closed(&events);
    events
}

/// Every row announced in `file_started` gets exactly one `row_finished`, and
/// rows don't overlap.
fn assert_rows_are_closed(events: &[Value]) {
    let mut announced = Vec::new();
    let mut finished = Vec::new();
    let mut open = None;
    for event in events {
        match event["type"].as_str().unwrap() {
            "file_started" => {
                for table in event["tables"].as_array().unwrap() {
                    for row in table["rows"].as_array().unwrap() {
                        announced.push((event["file"].clone(), row.clone()));
                    }
                }
            }
            "row_started" => {
                assert_eq!(None, open, "row started before the last one finished");
                open = Some(event["line"].clone());
            }
            "row_finished" => {
                open = None;
                finished.push((event["file"].clone(), event["line"].clone()));
            }
            _ => {}
        }
    }
    assert_eq!(announced, finished);
}

fn run(server: &str, args: &[&str]) -> (Vec<Value>, i32) {
    let output = temoc(server, args).output().unwrap();
    (events(&output), output.status.code().unwrap())
}

fn of_type<'a>(events: &'a [Value], kind: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| event["type"] == kind)
        .collect()
}

fn row_statuses(events: &[Value]) -> Vec<String> {
    of_type(events, "row_finished")
        .iter()
        .map(|row| row["status"].as_str().unwrap().to_string())
        .collect()
}

fn run_status(events: &[Value]) -> &Value {
    &events.last().unwrap()["status"]
}

const VALUES: &str = "| value | value? |\n|---|---|\n";

#[test]
fn all_pass() {
    let dir = workspace();
    let file = write(
        &dir,
        "a.md",
        &table("Probe", &format!("{VALUES}| 1 | 1 |\n| 2 | 2 |")),
    );
    let (events, code) = run(&format!("{} %p", probe()), &[&file]);
    assert_eq!(0, code);
    assert_eq!(vec!["passed", "passed"], row_statuses(&events));
    assert_eq!("passed", *run_status(&events));
    let files = of_type(&events, "file_started");
    assert_eq!(json!([5, 6]), files[0]["tables"][0]["rows"]);
    assert_eq!(json!(["value", "value?"]), files[0]["tables"][0]["columns"]);
    assert_eq!(1, files[0]["tables"][0]["line"]);
    assert_eq!(
        json!([
            {"role":"setter","method":"setValue","column":3,"status":"passed","value":"1"},
            {"role":"getter","method":"value","column":7,"status":"passed","expected":"1",
             "actual":{"kind":"value","value":"1"}},
        ]),
        of_type(&events, "row_finished")[0]["checks"]
    );
    assert_eq!(json!({"passed":2}), events.last().unwrap()["counts"],);
}

#[test]
fn one_wrong_value_fails_only_that_row() {
    let dir = workspace();
    let file = write(
        &dir,
        "a.md",
        &table("Probe", &format!("{VALUES}| 1 | 1 |\n| 2 | 3 |\n| 4 | 4 |")),
    );
    let (events, code) = run(&format!("{} %p", probe()), &[&file]);
    assert_eq!(1, code);
    assert_eq!(vec!["passed", "failed", "passed"], row_statuses(&events));
    let check = &of_type(&events, "row_finished")[1]["checks"][1];
    assert_eq!("3", check["expected"]);
    assert_eq!(json!({"kind":"value","value":"2"}), check["actual"]);
    assert_eq!(7, check["column"]);
    assert_eq!("failed", *run_status(&events));
}

#[test]
fn snoozes_are_reported_with_their_date() {
    let dir = workspace();
    let active = write(
        &dir,
        "active.md",
        &table(
            "Probe -- snooze until 2099-12-31",
            &format!("{VALUES}| 1 | 2 |"),
        ),
    );
    let (events, code) = run(&format!("{} %p", probe()), &[&active]);
    assert_eq!(0, code);
    assert_eq!(vec!["snoozed"], row_statuses(&events));
    assert_eq!(
        "2099-12-31",
        of_type(&events, "row_finished")[0]["checks"][1]["snoozed_until"]
    );
    assert_eq!("snoozed", *run_status(&events));
    let (with_flag, _) = run(&format!("{} %p", probe()), &[&active, "-s"]);
    assert_eq!(vec!["snoozed"], row_statuses(&with_flag));

    let expired = write(
        &dir,
        "expired.md",
        &table(
            "Probe -- snooze until 2000-01-01",
            &format!("{VALUES}| 1 | 2 |"),
        ),
    );
    let (events, code) = run(&format!("{} %p", probe()), &[&expired]);
    assert_eq!(1, code);
    assert_eq!(vec!["failed"], row_statuses(&events));
    assert_eq!(
        "2000-01-01",
        of_type(&events, "row_finished")[0]["checks"][1]["snoozed_until"]
    );
}

#[test]
fn real_and_snoozed_failures_keep_their_own_status() {
    let dir = workspace();
    let content = table("Probe", &format!("{VALUES}| 1 | 2 |"))
        + &table(
            "Probe -- snooze until 2099-12-31",
            &format!("{VALUES}| 1 | 2 |"),
        );
    let file = write(&dir, "a.md", &content);
    let (events, code) = run(&format!("{} %p", probe()), &[&file]);
    assert_eq!(1, code);
    assert_eq!(vec!["failed", "snoozed"], row_statuses(&events));
    assert_eq!("failed", *run_status(&events));
}

#[test]
fn rows_arrive_while_the_run_is_in_progress() {
    let dir = workspace();
    let file = write(
        &dir,
        "a.md",
        &table("Probe", "| slow? |\n|---|\n| done |\n| done |\n| done |"),
    );
    let mut child = temoc(&format!("{} %p", probe()), &[&file])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut arrivals = Vec::new();
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let event: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if event["type"] == "row_finished" {
            arrivals.push(Instant::now());
        }
    }
    child.wait().unwrap();
    assert_eq!(3, arrivals.len());
    assert!(arrivals[1] - arrivals[0] >= Duration::from_millis(300));
    assert!(arrivals[2] - arrivals[1] >= Duration::from_millis(300));
}

#[test]
fn exceptions_are_row_errors_and_later_rows_run() {
    let dir = workspace();
    let content = table("Probe", "| boom? |\n|---|\n| x |")
        + &table("Probe", "| broken | value? |\n|---|---|\n| 1 |  |")
        + &table("Probe", &format!("{VALUES}| 1 | 1 |"));
    let file = write(&dir, "a.md", &content);
    let (events, code) = run(&format!("{} %p", probe()), &[&file]);
    assert_eq!(1, code);
    assert_eq!(vec!["error", "error", "passed"], row_statuses(&events));
    let rows = of_type(&events, "row_finished");
    assert_eq!("exception", rows[0]["checks"][0]["actual"]["kind"]);
    assert_eq!("setter", rows[1]["checks"][0]["role"]);
    assert_eq!("error", rows[1]["checks"][0]["status"]);
    assert_eq!("error", *run_status(&events));
}

#[test]
fn unknown_symbols_are_row_errors_and_everything_else_still_runs() {
    let dir = workspace();
    let content = table(
        "Probe",
        &format!("{VALUES}| 1 | $NOPE |\n| $NOPE | 1 |\n| 2 | 2 |"),
    );
    let bad = write(&dir, "a.md", &content);
    let good = write(&dir, "b.md", &table("Probe", &format!("{VALUES}| 1 | 1 |")));
    let (events, code) = run(&format!("{} %p", probe()), &[&bad, &good]);
    assert_eq!(1, code);
    assert_eq!(
        vec!["error", "error", "passed", "passed"],
        row_statuses(&events)
    );
    let rows = of_type(&events, "row_finished");
    let missing = json!({"kind":"missing_symbol","symbol":"NOPE"});
    assert_eq!(missing, rows[0]["checks"][1]["actual"]);
    assert_eq!(missing, rows[1]["checks"][0]["actual"]);
    assert_eq!("skipped", rows[1]["checks"][1]["status"]);
}

#[test]
fn symbols_work_across_batches() {
    let dir = workspace();
    let content = table("Probe", &format!("{VALUES}| 7 | $V= |\n| $V | $V |"))
        + &table("Probe", "| value | number? |\n|---|---|\n| $V | 8 |");
    let file = write(&dir, "a.md", &content);
    let (events, code) = run(&format!("{} %p", probe()), &[&file]);
    assert_eq!(0, code, "{events:#?}");
    assert_eq!(vec!["passed", "passed", "passed"], row_statuses(&events));
    let rows = of_type(&events, "row_finished");
    let seven = json!({"kind":"value","value":"7"});
    assert_eq!("V", rows[0]["checks"][1]["assigned"]);
    assert_eq!("$V=", rows[0]["checks"][1]["expected"]);
    assert_eq!(seven, rows[0]["checks"][1]["actual"]);
    assert_eq!(seven, rows[1]["checks"][0]["resolved"]);
    assert_eq!("$V", rows[1]["checks"][1]["expected"]);
    assert_eq!(seven, rows[1]["checks"][1]["resolved"]);
}

#[test]
fn a_failed_make_skips_the_rows_of_that_table_only() {
    let dir = workspace();
    let content = table("NoSuchClass", &format!("{VALUES}| 1 | 1 |"))
        + &table("Probe", &format!("{VALUES}| 1 | 1 |"));
    let file = write(&dir, "a.md", &content);
    let (events, code) = run(&format!("{} %p", probe()), &[&file]);
    assert_eq!(1, code);
    assert_eq!(vec!["skipped", "passed"], row_statuses(&events));
    let setup = of_type(&events, "setup_failed");
    assert_eq!(1, setup.len());
    assert_eq!("make", setup[0]["phase"]);
    assert_eq!(1, setup[0]["line"]);
    assert_eq!("exception", setup[0]["actual"]["kind"]);
    assert_eq!("error", of_type(&events, "table_finished")[0]["status"]);
}

#[test]
fn abort_slim_test_skips_the_rest_of_the_file_only() {
    let dir = workspace();
    let content = table("Probe", "| abort test? |\n|---|\n| x |\n| x |")
        + &table("Probe", &format!("{VALUES}| 1 | 1 |"));
    let aborted = write(&dir, "a.md", &content);
    let next = write(&dir, "b.md", &table("Probe", &format!("{VALUES}| 1 | 1 |")));
    let (events, code) = run(&format!("{} %p", probe()), &[&aborted, &next]);
    assert_eq!(1, code);
    assert_eq!(
        vec!["error", "skipped", "skipped", "passed"],
        row_statuses(&events)
    );
    assert_eq!(
        0,
        of_type(&events, "row_started")
            .iter()
            .filter(|row| row["line"] == 6)
            .count()
    );
}

#[test]
fn abort_slim_suite_stops_the_run() {
    let dir = workspace();
    let aborted = write(
        &dir,
        "a.md",
        &table("Probe", "| abort suite? |\n|---|\n| x |\n| x |"),
    );
    let next = write(&dir, "b.md", &table("Probe", &format!("{VALUES}| 1 | 1 |")));
    let (events, code) = run(&format!("{} %p", probe()), &[&aborted, &next]);
    assert_eq!(1, code);
    assert_eq!(vec!["error", "skipped"], row_statuses(&events));
    assert_eq!(1, of_type(&events, "file_started").len());
    assert_eq!("suite_aborted", of_type(&events, "run_error")[0]["kind"]);
}

#[test]
fn ignore_script_test_skips_the_rest_of_the_table() {
    let dir = workspace();
    let content = table("Probe", "| ignore script? |\n|---|\n| x |\n| x |")
        + &table("Probe", &format!("{VALUES}| 1 | 1 |"));
    let file = write(&dir, "a.md", &content);
    let (events, code) = run(&format!("{} %p", probe()), &[&file]);
    assert_eq!(0, code);
    assert_eq!(vec!["skipped", "skipped", "passed"], row_statuses(&events));
}

#[test]
fn ignore_all_tests_skips_the_rest_of_the_file() {
    let dir = workspace();
    let content = table("Probe", "| ignore all? |\n|---|\n| x |\n| x |")
        + &table("Probe", &format!("{VALUES}| 1 | 1 |"));
    let file = write(&dir, "a.md", &content);
    let next = write(&dir, "b.md", &table("Probe", &format!("{VALUES}| 1 | 1 |")));
    let (events, code) = run(&format!("{} %p", probe()), &[&file, &next]);
    assert_eq!(0, code);
    assert_eq!(
        vec!["skipped", "skipped", "skipped", "passed"],
        row_statuses(&events)
    );
}

#[test]
fn a_server_that_will_not_start_is_reported_with_its_stderr() {
    let dir = workspace();
    let file = write(&dir, "a.md", &table("Probe", &format!("{VALUES}| 1 | 1 |")));
    let server = "echo cannot start >&2; exit 7";
    let (events, code) = run(server, &[&file]);
    assert_eq!(1, code);
    assert_eq!(vec!["skipped"], row_statuses(&events));
    let error = of_type(&events, "run_error")[0];
    assert_eq!("server_start_failed", error["kind"]);
    assert_eq!("cannot start\n", error["stderr"]);
    assert_eq!("run_error", events[events.len() - 2]["type"]);
    assert_eq!("error", *run_status(&events));
}

#[test]
fn a_tcp_server_that_will_not_start_fails_fast() {
    let dir = workspace();
    let file = write(&dir, "a.md", &table("Probe", &format!("{VALUES}| 1 | 1 |")));
    let started = Instant::now();
    let output = Command::new(TEMOC)
        .args(["--output", "jsonl", "-x", "exit 7", "-p", "18300", &file])
        .output()
        .unwrap();
    let events = events(&output);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        "server_start_failed",
        of_type(&events, "run_error")[0]["kind"]
    );
}

#[test]
fn a_server_that_dies_mid_run_is_reported_without_a_panic() {
    let dir = workspace();
    let content = table("Probe", "| value | die? |\n|---|---|\n| 1 | x |\n| 2 | x |");
    let file = write(&dir, "a.md", &content);
    let next = write(&dir, "b.md", &table("Probe", &format!("{VALUES}| 1 | 1 |")));
    let output = temoc(&format!("{} %p", probe()), &[&file, &next])
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    let events = events(&output);
    assert_eq!(Some(1), output.status.code());
    assert_eq!(vec!["error", "skipped"], row_statuses(&events));
    let error = of_type(&events, "run_error")[0];
    assert_eq!("server_died", error["kind"]);
    assert!(error["message"].as_str().unwrap().contains("exit"));
    assert_eq!(1, of_type(&events, "file_started").len());
}

#[test]
fn a_selector_that_matches_nothing_is_an_error_but_the_next_file_runs() {
    let dir = workspace();
    let file = write(&dir, "a.md", &table("Probe", &format!("{VALUES}| 1 | 1 |")));
    let (events, code) = run(&format!("{} %p", probe()), &[&format!("{file}:999"), &file]);
    assert_eq!(1, code);
    assert_eq!("no_match", of_type(&events, "run_error")[0]["kind"]);
    assert_eq!(vec!["passed"], row_statuses(&events));
}

#[test]
fn a_row_selector_runs_only_that_row() {
    let dir = workspace();
    let file = write(
        &dir,
        "a.md",
        &table("Probe", &format!("{VALUES}| 1 | 1 |\n| 2 | 9 |\n| 3 | 3 |")),
    );
    let (events, code) = run(&format!("{} %p", probe()), &[&format!("{file}:5")]);
    assert_eq!(0, code);
    assert_eq!(vec!["passed"], row_statuses(&events));
    assert_eq!(5, of_type(&events, "row_finished")[0]["line"]);
    assert_eq!(
        json!([5]),
        of_type(&events, "file_started")[0]["tables"][0]["rows"]
    );
}

#[test]
fn a_row_run_alone_reports_a_symbol_set_elsewhere() {
    let dir = workspace();
    let file = write(
        &dir,
        "a.md",
        &table("Probe", &format!("{VALUES}| 1 | $V= |\n| $V | 1 |")),
    );
    let (events, code) = run(&format!("{} %p", probe()), &[&format!("{file}:6")]);
    assert_eq!(1, code);
    assert_eq!(vec!["error"], row_statuses(&events));
    assert_eq!(
        json!({"kind":"missing_symbol","symbol":"V"}),
        of_type(&events, "row_finished")[0]["checks"][0]["actual"]
    );
}

#[test]
fn server_output_becomes_events_on_stdio() {
    let dir = workspace();
    let file = write(&dir, "a.md", &table("Probe", "| noisy? |\n|---|\n| done |"));
    let (events, code) = run(&format!("{} %p", probe()), &[&file, "-o"]);
    assert_eq!(0, code);
    let output = of_type(&events, "output");
    assert_eq!("to stderr\n", output[0]["text"]);
    assert_eq!("stderr", output[0]["stream"]);
    assert_eq!(file, output[0]["file"]);
}

#[test]
fn server_output_becomes_events_on_tcp() {
    let dir = workspace();
    let file = write(
        &dir,
        "a.md",
        &table("Probe", "| shout? | noisy? |\n|---|---|\n| done | done |"),
    );
    let output = Command::new(TEMOC)
        .args(["--output", "jsonl", "-o", "-p", "18320", "-l", "10", "-x"])
        .arg(format!("{} %p", probe()))
        .arg(&file)
        .output()
        .unwrap();
    let events = events(&output);
    assert_eq!(Some(0), output.status.code());
    let streams: Vec<_> = of_type(&events, "output")
        .iter()
        .map(|line| (line["stream"].clone(), line["text"].clone()))
        .collect();
    assert!(streams.contains(&(json!("stdout"), json!("to stdout\n"))));
    assert!(streams.contains(&(json!("stderr"), json!("to stderr\n"))));
}

#[test]
fn method_tables_are_compared_cell_by_cell() {
    let dir = workspace();
    let file = write(
        &dir,
        "a.md",
        &table(
            "Probe#pair",
            "| a? | b? |\n|---|---|\n| left | right |\n| left | wrong |",
        ),
    );
    let (events, code) = run(&format!("{} %p", probe()), &[&file]);
    assert_eq!(1, code);
    assert_eq!(vec!["passed", "failed"], row_statuses(&events));
    let checks = &of_type(&events, "row_finished")[1]["checks"];
    assert_eq!("passed", checks[0]["status"]);
    assert_eq!("failed", checks[1]["status"]);
    assert_eq!("wrong", checks[1]["expected"]);
    assert_eq!(10, checks[1]["column"]);
    assert_eq!(json!({"kind":"value","value":"right"}), checks[1]["actual"]);
}

#[test]
fn a_method_row_with_the_wrong_length_has_no_column() {
    let dir = workspace();
    let file = write(
        &dir,
        "a.md",
        &table("Probe#pair", "| a? |\n|---|\n| left |"),
    );
    let (events, code) = run(&format!("{} %p", probe()), &[&file]);
    assert_eq!(1, code);
    let check = &of_type(&events, "row_finished")[0]["checks"][0];
    assert_eq!("failed", check["status"]);
    assert_eq!(Value::Null, check["column"]);
}

fn wait_for_row_started(child: &mut Child) -> BufReader<std::process::ChildStdout> {
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    while !line.contains("row_started") {
        line.clear();
        assert_ne!(0, reader.read_line(&mut line).unwrap());
    }
    reader
}

fn process_exists(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn cancel_mid_run(server: &str, signal: &str) {
    let dir = workspace();
    let pid_file = dir.join("pid");
    let server = format!("echo $$ > {}; {server}", pid_file.display());
    let file = write(
        &dir,
        "a.md",
        &table("Probe", "| hang? |\n|---|\n| x |\n| x |"),
    );
    let mut child = temoc(&server, &[&file])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let reader = wait_for_row_started(&mut child);
    let server_pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
    assert!(process_exists(&server_pid));
    Command::new("kill")
        .args([signal, &child.id().to_string()])
        .status()
        .unwrap();
    let started = Instant::now();
    let lines: Vec<Value> = reader
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect();
    let status = child.wait().unwrap();
    assert!(started.elapsed() < Duration::from_secs(8));
    assert_eq!(Some(1), status.code());
    let last = lines.last().unwrap();
    assert_eq!("run_finished", last["type"]);
    assert_eq!("cancelled", last["status"]);
    assert_eq!(vec!["skipped", "skipped"], row_statuses(&lines));
    assert!(!process_exists(&server_pid), "server left running");
}

#[test]
fn sigterm_cancels_and_leaves_no_server_behind() {
    cancel_mid_run(&format!("exec {} %p", probe()), "-TERM");
}

#[test]
fn sigint_cancels_and_leaves_no_server_behind() {
    cancel_mid_run(&format!("exec {} %p", probe()), "-INT");
}

#[test]
fn a_server_that_ignores_sigterm_is_killed() {
    cancel_mid_run(&format!("trap '' TERM; exec {} %p", probe()), "-TERM");
}

#[test]
fn output_can_be_set_in_the_config_file() {
    let dir = workspace();
    let file = write(&dir, "a.md", &table("Probe", &format!("{VALUES}| 1 | 1 |")));
    let config = write(
        &dir,
        "temoc.toml",
        &format!(
            "output = \"jsonl\"\nexecute_server_command = \"{} %p\"\nport = 1\n",
            probe()
        ),
    );
    let output = Command::new(TEMOC)
        .args(["-c", &config, &file])
        .output()
        .unwrap();
    assert_eq!(vec!["passed"], row_statuses(&events(&output)));

    let output = Command::new(TEMOC)
        .args(["-c", &config, "--output", "text", &file])
        .output()
        .unwrap();
    assert_eq!(
        format!("Testing file {file}...OK\n"),
        String::from_utf8(output.stdout).unwrap()
    );
}

#[test]
fn bad_config_is_an_event() {
    let dir = workspace();
    let config = write(
        &dir,
        "temoc.toml",
        "output = \"jsonl\"\nrecursive = \"yes\"\n",
    );
    let output = Command::new(TEMOC).args(["-c", &config]).output().unwrap();
    let events = events(&output);
    assert_eq!(Some(1), output.status.code());
    assert_eq!("bad_config", of_type(&events, "run_error")[0]["kind"]);

    let output = Command::new(TEMOC)
        .args(["--output", "jsonl", "a.md"])
        .output()
        .unwrap();
    assert_eq!(
        "bad_config",
        of_type(&self::events(&output), "run_error")[0]["kind"]
    );
}

#[test]
fn a_file_that_does_not_parse_does_not_stop_the_others() {
    let dir = workspace();
    let broken = write(
        &dir,
        "a.md",
        "[//]: # \"decisionTable Probe\"\n\nNot a table.\n",
    );
    let good = write(&dir, "b.md", &table("Probe", &format!("{VALUES}| 1 | 1 |")));
    let (events, code) = run(&format!("{} %p", probe()), &[&broken, "missing.md", &good]);
    assert_eq!(1, code);
    let kinds: Vec<_> = of_type(&events, "run_error")
        .iter()
        .map(|error| error["kind"].clone())
        .collect();
    assert_eq!(vec![json!("parse_error"), json!("file_error")], kinds);
    assert_eq!(vec!["passed"], row_statuses(&events));
}

#[test]
fn a_file_without_tables_passes() {
    let dir = workspace();
    let file = write(&dir, "a.md", "# Just text\n");
    let (events, code) = run("exit 1", &[&file]);
    assert_eq!(0, code);
    assert_eq!(json!([]), of_type(&events, "file_started")[0]["tables"]);
    assert_eq!("passed", *run_status(&events));
}

#[test]
fn stopping_the_reader_stops_the_run() {
    let dir = workspace();
    let file = write(
        &dir,
        "a.md",
        &table("Probe", "| slow? |\n|---|\n| done |\n| done |\n| done |"),
    );
    let mut child = temoc(&format!("{} %p", probe()), &[&file])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    drop(wait_for_row_started(&mut child));
    assert_eq!(Some(1), child.wait().unwrap().code());
}

#[test]
fn text_output_is_unchanged_for_failures() {
    let dir = workspace();
    let file = write(
        &dir,
        "a.md",
        &table("Probe", &format!("{VALUES}| 1 | 1 |\n| 2 | 3 |")),
    );
    let output = Command::new(TEMOC)
        .args(["-p", "1", "-x"])
        .arg(format!("{} %p", probe()))
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(Some(1), output.status.code());
    assert_eq!(
        format!(
            "Testing file {file}...FAIL\nExpected `3`, got `2` in {file}:6:7 for method call value\n"
        ),
        String::from_utf8(output.stdout).unwrap()
    );
}
