#![cfg(feature = "conformance-tests")]

use slim_protocol::{
    ExceptionMessage, Id, Instruction, InstructionResult, InstructionResultValue, SlimConnection,
    SlimValue,
};
use std::{
    io::Read,
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn id(value: &str) -> Id {
    Id::from(value)
}

fn string(value: &str) -> InstructionResultValue {
    InstructionResultValue::String(value.into())
}

fn exception(value: &str) -> InstructionResultValue {
    InstructionResultValue::Exception(ExceptionMessage::new(value.into()))
}

fn server_command(port: u16) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_slim_conformance_server"));
    command.arg(port.to_string());
    command
}

fn unused_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve an ephemeral port");
    listener
        .local_addr()
        .expect("get the ephemeral port")
        .port()
}

fn connect(port: u16) -> TcpStream {
    let started = Instant::now();
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => return stream,
            Err(error) if started.elapsed() < Duration::from_secs(5) => {
                let _ = error;
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("connect to conformance server: {error}"),
        }
    }
}

fn close_child(mut child: Child) {
    let status = child.wait().expect("wait for conformance server");
    assert!(status.success(), "conformance server exited with {status}");
}

#[test]
fn tcp_v05_conformance_exercises_recursive_values_and_execution_context() {
    let port = unused_port();
    let child = server_command(port)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start TCP conformance server");
    let stream = connect(port);
    let mut connection = SlimConnection::new(stream.try_clone().unwrap(), stream).unwrap();

    let nested = SlimValue::List(vec![
        SlimValue::String("é".into()),
        SlimValue::List(vec![SlimValue::String("😀".into())]),
    ]);
    let batch = vec![
        Instruction::Import {
            id: id("import"),
            path: "Conformance".into(),
        },
        Instruction::Make {
            id: id("make"),
            instance: "fixture".into(),
            class: "Fixture".into(),
            args: vec![SlimValue::String("é😀".into())],
        },
        Instruction::Call {
            id: id("initial"),
            instance: "fixture".into(),
            function: "initial".into(),
            args: vec![],
        },
        Instruction::Call {
            id: id("nested"),
            instance: "fixture".into(),
            function: "nested".into(),
            args: vec![nested.clone()],
        },
        Instruction::CallAndAssign {
            id: id("saved"),
            symbol: "Saved".into(),
            instance: "fixture".into(),
            function: "echo".into(),
            args: vec![SlimValue::String("value".into())],
        },
        Instruction::Call {
            id: id("symbol"),
            instance: "fixture".into(),
            function: "echo".into(),
            args: vec![SlimValue::String("prefix-$Saved".into())],
        },
        Instruction::Assign {
            id: id("null"),
            symbol: "Nil".into(),
            value: SlimValue::String("null".into()),
        },
        Instruction::Call {
            id: id("null-use"),
            instance: "fixture".into(),
            function: "echo".into(),
            args: vec![SlimValue::String("$Nil".into())],
        },
        Instruction::CallAndAssign {
            id: id("object"),
            symbol: "Object".into(),
            instance: "fixture".into(),
            function: "object".into(),
            args: vec![],
        },
        Instruction::Make {
            id: id("copy"),
            instance: "copy".into(),
            class: "$Object".into(),
            args: vec![],
        },
        Instruction::Call {
            id: id("chain"),
            instance: "copy".into(),
            function: "ping".into(),
            args: vec![],
        },
        Instruction::Make {
            id: id("lib-one"),
            instance: "libraryOne".into(),
            class: "LibraryOne".into(),
            args: vec![],
        },
        Instruction::Make {
            id: id("lib-two"),
            instance: "libraryTwo".into(),
            class: "LibraryTwo".into(),
            args: vec![],
        },
        Instruction::Call {
            id: id("library"),
            instance: "fixture".into(),
            function: "libraryValue".into(),
            args: vec![],
        },
        Instruction::CallAndAssign {
            id: id("actor"),
            symbol: "Actor".into(),
            instance: "fixture".into(),
            function: "actor".into(),
            args: vec![],
        },
        Instruction::Make {
            id: id("actor-make"),
            instance: "scriptTableActor".into(),
            class: "$Actor".into(),
            args: vec![],
        },
        Instruction::Call {
            id: id("push"),
            instance: "SlimHelperLibrary".into(),
            function: "pushFixture".into(),
            args: vec![],
        },
        Instruction::Make {
            id: id("replace"),
            instance: "scriptTableActor".into(),
            class: "Fixture".into(),
            args: vec![SlimValue::String("replacement".into())],
        },
        Instruction::Call {
            id: id("pop"),
            instance: "SlimHelperLibrary".into(),
            function: "popFixture".into(),
            args: vec![],
        },
        Instruction::Call {
            id: id("actor-name"),
            instance: "scriptTableActor".into(),
            function: "name".into(),
            args: vec![],
        },
        Instruction::Call {
            id: id("reset"),
            instance: "fixture".into(),
            function: "reset".into(),
            args: vec![],
        },
        Instruction::Make {
            id: id("no-constructor"),
            instance: "none".into(),
            class: "Fixture".into(),
            args: vec![],
        },
        Instruction::Make {
            id: id("constructor-failure"),
            instance: "failed".into(),
            class: "Fixture".into(),
            args: vec![SlimValue::String("fail".into())],
        },
        Instruction::Make {
            id: id("converter-failure"),
            instance: "bad".into(),
            class: "Fixture".into(),
            args: vec![SlimValue::List(vec![])],
        },
        Instruction::Call {
            id: id("missing"),
            instance: "fixture".into(),
            function: "missing".into(),
            args: vec![],
        },
        Instruction::Call {
            id: id("fixture-failure"),
            instance: "fixture".into(),
            function: "fail".into(),
            args: vec![],
        },
        Instruction::Call {
            id: id("sut"),
            instance: "fixture".into(),
            function: "answer".into(),
            args: vec![],
        },
        Instruction::Call {
            id: id("clone-symbol"),
            instance: "SlimHelperLibrary".into(),
            function: "cloneSymbol".into(),
            args: vec![SlimValue::String("$Saved".into())],
        },
        Instruction::Assign {
            id: id("assign-list"),
            symbol: "List".into(),
            value: nested,
        },
        Instruction::Make {
            id: id("copy-list"),
            instance: "listInstance".into(),
            class: "$List".into(),
            args: vec![],
        },
        Instruction::Make {
            id: id("copy-null"),
            instance: "nullInstance".into(),
            class: "$Nil".into(),
            args: vec![],
        },
        Instruction::Call {
            id: id("missing-instance"),
            instance: "missing".into(),
            function: "libraryValue".into(),
            args: vec![],
        },
    ];
    let results = connection.send_instructions(&batch).unwrap();
    assert_eq!(batch.len(), results.len());
    assert_eq!(InstructionResult::ok(id("import")).value, results[0].value);
    assert_eq!(InstructionResult::ok(id("make")).value, results[1].value);
    assert_eq!(string("é😀"), results[2].value);
    assert_eq!(
        InstructionResultValue::List(vec![
            string("é"),
            InstructionResultValue::List(vec![string("😀")])
        ]),
        results[3].value
    );
    assert_eq!(string("value"), results[4].value);
    assert_eq!(string("prefix-value"), results[5].value);
    assert_eq!(InstructionResult::ok(id("null")).value, results[6].value);
    assert_eq!(string("null"), results[7].value);
    assert_eq!(string("chained"), results[8].value);
    assert_eq!(InstructionResult::ok(id("copy")).value, results[9].value);
    assert_eq!(string("chained"), results[10].value);
    assert_eq!(string("two"), results[13].value);
    assert_eq!(string("actor"), results[19].value);
    assert_eq!(InstructionResultValue::Void, results[20].value);
    assert_eq!(
        exception("message:<<NO_CONSTRUCTOR Fixture>>"),
        results[21].value
    );
    assert_eq!(
        exception("message:<<COULD_NOT_INVOKE_CONSTRUCTOR Fixture requested failure>>"),
        results[22].value
    );
    assert_eq!(
        exception("message:<<NO_CONVERTER_FOR_ARGUMENT_NUMBER String>>"),
        results[23].value
    );
    assert_eq!(
        exception("message:<<NO_METHOD_IN_CLASS missing Conformance.Fixture>>"),
        results[24].value
    );
    assert_eq!(exception("fixture failed"), results[25].value);
    assert_eq!(string("sut"), results[26].value);
    assert_eq!(string("value"), results[27].value);
    assert_eq!(
        InstructionResult::ok(id("assign-list")).value,
        results[28].value
    );
    assert_eq!(
        InstructionResult::ok(id("copy-list")).value,
        results[29].value
    );
    assert_eq!(
        InstructionResult::ok(id("copy-null")).value,
        results[30].value
    );
    assert_eq!(
        exception("message:<<NO_INSTANCE missing>>"),
        results[31].value
    );

    let stopped = connection
        .send_instructions(&[
            Instruction::Call {
                id: id("stop"),
                instance: "fixture".into(),
                function: "stop".into(),
                args: vec![],
            },
            Instruction::Call {
                id: id("skipped"),
                instance: "fixture".into(),
                function: "initial".into(),
                args: vec![],
            },
        ])
        .unwrap();
    assert_eq!(
        vec![InstructionResult::exception(
            id("stop"),
            ExceptionMessage::new("ABORT_SLIM_TEST:message:<<stop batch>>".into())
        )],
        stopped
    );
    let next_batch = connection
        .send_instructions(&[Instruction::Call {
            id: id("next"),
            instance: "fixture".into(),
            function: "initial".into(),
            args: vec![],
        }])
        .unwrap();
    assert_eq!(
        vec![InstructionResult::string(id("next"), "é😀".into())],
        next_batch
    );
    connection.close().unwrap();
    close_child(child);
}

#[test]
fn stdio_port_one_keeps_protocol_frames_clean_and_tunnels_explicit_output() {
    let mut child = server_command(1)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start stdio conformance server");
    let stderr = child.stderr.take().unwrap();
    let stderr_reader = thread::spawn(move || {
        let mut output = String::new();
        std::io::BufReader::new(stderr)
            .read_to_string(&mut output)
            .unwrap();
        output
    });
    let stdout = child.stdout.take().unwrap();
    let stdin = child.stdin.take().unwrap();
    let mut connection = SlimConnection::new(stdout, stdin).unwrap();
    let results = connection
        .send_instructions(&[
            Instruction::Make {
                id: id("make"),
                instance: "fixture".into(),
                class: "Conformance.Fixture".into(),
                args: vec![SlimValue::String("stdio".into())],
            },
            Instruction::Call {
                id: id("tunnel"),
                instance: "fixture".into(),
                function: "tunnel".into(),
                args: vec![],
            },
            Instruction::Call {
                id: id("nested"),
                instance: "fixture".into(),
                function: "nested".into(),
                args: vec![SlimValue::List(vec![SlimValue::List(vec![
                    SlimValue::String("😀".into()),
                ])])],
            },
        ])
        .unwrap();
    assert_eq!(InstructionResultValue::Void, results[1].value);
    assert_eq!(
        InstructionResultValue::List(vec![InstructionResultValue::List(vec![string("😀")])]),
        results[2].value
    );
    let second_batch = connection
        .send_instructions(&[Instruction::Call {
            id: id("stdio-second-batch"),
            instance: "fixture".into(),
            function: "initial".into(),
            args: vec![],
        }])
        .unwrap();
    assert_eq!(
        vec![InstructionResult::string(
            id("stdio-second-batch"),
            "stdio".into()
        )],
        second_batch
    );
    connection.close().unwrap();
    close_child(child);
    assert_eq!(
        "SOUT :first\nSOUT.:second\nSERR :error\nSERR.:detail\n",
        stderr_reader.join().unwrap()
    );
}

#[cfg(feature = "html-hash")]
#[test]
fn tcp_html_hash_conversion_runs_through_construction_and_calls() {
    let port = unused_port();
    let child = server_command(port)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start TCP conformance server");
    let stream = connect(port);
    let mut connection = SlimConnection::new(stream.try_clone().unwrap(), stream).unwrap();
    let table = "<table><tr><td>name</td><td>one &amp; two</td></tr></table>";
    let results = connection
        .send_instructions(&[
            Instruction::Make {
                id: id("make-hash"),
                instance: "fixture".into(),
                class: "Conformance.Fixture".into(),
                args: vec![SlimValue::String(table.into())],
            },
            Instruction::Call {
                id: id("constructed-hash"),
                instance: "fixture".into(),
                function: "initial".into(),
                args: vec![],
            },
            Instruction::Call {
                id: id("called-hash"),
                instance: "fixture".into(),
                function: "hash".into(),
                args: vec![SlimValue::String(table.into())],
            },
        ])
        .unwrap();
    assert_eq!(string("hash:one & two"), results[1].value);
    assert_eq!(string(table), results[2].value);
    connection.close().unwrap();
    close_child(child);
}
