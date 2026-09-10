use crate::{
    ClassPath, Constructor, ConstructorError, ExecuteMethodError, SlimControl,
    SlimControlException, SlimFixture, SlimObject, SlimValue,
};
use convert_case::{Case, Casing};
use slim_protocol::{
    ByeOrSlimInstructions, ExceptionMessage, FromSlimReader, FromSlimReaderError, Id, Instruction,
    InstructionResult, InstructionResultValue, SlimValue as WireSlimValue, ToSlimString,
};
use std::{
    collections::HashMap,
    io::{BufReader, Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};
use thiserror::Error;

pub type SlimClosureConstructor =
    Box<dyn Fn(Vec<SlimValue>) -> Result<SlimObject, ConstructorError>>;

#[derive(Debug, Error)]
pub enum SlimServerError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    FromSlimReaderError(#[from] FromSlimReaderError),
    #[error(transparent)]
    Options(#[from] SlimServerOptionsError),
}

/// Server configuration understood by the V0.5 runtime.
///
/// `instruction_timeout` is an *observational* timeout. Fixtures execute on
/// the server thread and may own `Rc<RefCell<_>>` state, so Rust cannot safely
/// cancel arbitrary fixture code. Once an instruction returns, its elapsed
/// time is checked and an overrun is reported as `TIMED_OUT <seconds>`.
/// Side effects performed before the return remain visible and the runtime
/// continues with the next instruction. Fixtures that need prompt stopping
/// must implement cooperative cancellation in their own APIs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlimServerOptions {
    instruction_timeout: Option<Duration>,
}

impl SlimServerOptions {
    pub fn with_instruction_timeout_seconds(seconds: u64) -> Self {
        Self {
            instruction_timeout: Some(Duration::from_secs(seconds)),
        }
    }

    pub fn instruction_timeout(&self) -> Option<Duration> {
        self.instruction_timeout
    }

    /// Parses the documented `-s <seconds>` SliM flag. Other arguments are
    /// ignored so the same slice can include the command-line port.
    pub fn from_slim_flags(flags: &str) -> Result<Self, SlimServerOptionsError> {
        Self::from_args(flags.split_whitespace())
    }

    pub fn from_args<I, S>(args: I) -> Result<Self, SlimServerOptionsError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let values = args
            .into_iter()
            .map(|value| value.as_ref().to_owned())
            .collect::<Vec<_>>();
        let mut options = Self::default();
        let mut index = 0;
        while index < values.len() {
            let value = &values[index];
            let seconds = if value == "-s" {
                index += 1;
                values
                    .get(index)
                    .ok_or(SlimServerOptionsError::MissingTimeout)?
            } else if let Some(seconds) = value.strip_prefix("-s") {
                if seconds.is_empty() {
                    index += 1;
                    values
                        .get(index)
                        .ok_or(SlimServerOptionsError::MissingTimeout)?
                } else {
                    seconds
                }
            } else {
                index += 1;
                continue;
            };
            let seconds = seconds
                .parse::<u64>()
                .map_err(|_| SlimServerOptionsError::InvalidTimeout(seconds.to_owned()))?;
            options.instruction_timeout = Some(Duration::from_secs(seconds));
            index += 1;
        }
        Ok(options)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SlimServerOptionsError {
    #[error("the -s SliM timeout flag needs a number of seconds")]
    MissingTimeout,
    #[error("invalid -s SliM timeout value: {0}")]
    InvalidTimeout(String),
    #[error("the SliM server command line needs a port")]
    MissingPort,
    #[error("invalid SliM server port: {0}")]
    InvalidPort(String),
}

/// The mutable execution context required by the SliM instruction set.
pub struct SlimServer<R: Read, W: Write> {
    fixtures: HashMap<String, SlimClosureConstructor>,
    instances: HashMap<String, SlimObject>,
    /// Kept as a vector because library lookup is a stack, not a map.
    libraries: Vec<(String, SlimObject)>,
    actors: Vec<SlimObject>,
    symbols: HashMap<String, SlimValue>,
    imports: Vec<String>,
    reader: BufReader<R>,
    writer: W,
    options: SlimServerOptions,
}

impl<R: Read, W: Write> SlimServer<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self::with_options(reader, writer, SlimServerOptions::default())
    }

    pub fn with_options(reader: R, writer: W, options: SlimServerOptions) -> Self {
        Self {
            fixtures: HashMap::new(),
            instances: HashMap::new(),
            libraries: Vec::new(),
            actors: Vec::new(),
            symbols: HashMap::new(),
            imports: Vec::new(),
            reader: BufReader::new(reader),
            writer,
            options,
        }
    }

    pub fn add_fixture<T: ClassPath + Constructor + SlimFixture + 'static>(&mut self) {
        self.fixtures.insert(
            T::class_path(),
            Box::new(|args| {
                T::construct(args).map(|fixture| SlimObject::fixture(fixture, T::class_path()))
            }),
        );
    }

    pub fn run(mut self) -> Result<(), SlimServerError> {
        self.writer.write_all(b"Slim -- V0.5\n")?;
        self.writer.flush()?;
        loop {
            match ByeOrSlimInstructions::from_reader(&mut self.reader)? {
                ByeOrSlimInstructions::Bye => break,
                ByeOrSlimInstructions::Instructions(instructions) => {
                    let result = self.execute_instructions(instructions);
                    self.writer.write_all(result.to_slim_string().as_bytes())?;
                    self.writer.flush()?;
                }
            }
        }
        Ok(())
    }

    fn execute_instructions(&mut self, instructions: Vec<Instruction>) -> Vec<InstructionResult> {
        let mut results = Vec::with_capacity(instructions.len());
        for instruction in instructions {
            let timeout_eligible = matches!(
                &instruction,
                Instruction::Make { .. }
                    | Instruction::Call { .. }
                    | Instruction::CallAndAssign { .. }
            );
            let started = Instant::now();
            let mut result = self.execute_instruction(instruction);
            if timeout_eligible && control_from_result(&result).is_none() {
                if let Some(timeout) = self.options.instruction_timeout {
                    if started.elapsed() >= timeout {
                        result = exception(
                            result.id.clone(),
                            format!("TIMED_OUT {}", timeout.as_secs()),
                        );
                    }
                }
            }
            let control = control_from_result(&result);
            results.push(result);
            if control.is_some() {
                break;
            }
        }
        results
    }

    fn execute_instruction(&mut self, instruction: Instruction) -> InstructionResult {
        match instruction {
            Instruction::Malformed { id, fields } => {
                exception(id, format!("MALFORMED_INSTRUCTION [{}]", fields.join(",")))
            }
            Instruction::Import { id, path } => {
                self.imports.push(path);
                InstructionResult::ok(id)
            }
            Instruction::Make {
                id,
                instance,
                class,
                args,
            } => self.make(id, instance, class, args),
            Instruction::Call {
                id,
                instance,
                function,
                args,
            } => self.call(id, None, instance, function, args),
            Instruction::CallAndAssign {
                id,
                symbol,
                instance,
                function,
                args,
            } => {
                if !is_symbol_name(&symbol) {
                    let mut fields = vec![
                        id.to_string(),
                        "callAndAssign".into(),
                        symbol,
                        instance,
                        function,
                    ];
                    fields.extend(args.iter().map(wire_value_text));
                    return malformed_instruction(id, fields);
                }
                self.call(id, Some(symbol), instance, function, args)
            }
            Instruction::Assign { id, symbol, value } => {
                if !is_symbol_name(&symbol) {
                    let fields = vec![
                        id.to_string(),
                        "assign".into(),
                        symbol,
                        wire_value_text(&value),
                    ];
                    return malformed_instruction(id, fields);
                }
                self.symbols
                    .insert(symbol, runtime_value_without_symbols(value));
                InstructionResult::ok(id)
            }
        }
    }

    fn make(
        &mut self,
        id: Id,
        instance: String,
        class: String,
        args: Vec<WireSlimValue>,
    ) -> InstructionResult {
        if let Some(SlimValue::Object(object)) = self.whole_symbol(&class) {
            self.insert_instance(instance, object);
            return InstructionResult::ok(id);
        }
        let class = self.replace_symbols(&class);
        let Some(constructor) = self.find_fixture(&class) else {
            return exception(id, format!("NO_CLASS {class}"));
        };
        let args = self.parse_wire_values(args);
        match constructor(args) {
            Ok(fixture) => {
                self.insert_instance(instance, fixture);
                InstructionResult::ok(id)
            }
            Err(ConstructorError::NoConstructor) => {
                exception(id, format!("NO_CONSTRUCTOR {class}"))
            }
            Err(ConstructorError::ArgumentParsingError(argument)) => {
                exception(id, format!("NO_CONVERTER_FOR_ARGUMENT_NUMBER {argument}"))
            }
            Err(ConstructorError::CouldNotInvoke(message)) => exception(
                id,
                format!("COULD_NOT_INVOKE_CONSTRUCTOR {class} message:<<{message}>>"),
            ),
            Err(ConstructorError::Control(control)) => control_exception(id, control),
        }
    }

    fn insert_instance(&mut self, name: String, object: SlimObject) {
        self.instances.insert(name.clone(), object.clone());
        if name.starts_with("library") {
            self.libraries.push((name, object));
        }
    }

    fn call(
        &mut self,
        id: Id,
        assigned_symbol: Option<String>,
        instance: String,
        function: String,
        args: Vec<WireSlimValue>,
    ) -> InstructionResult {
        let protocol_function = function;
        let function = protocol_function.to_case(Case::Snake);
        let args = self.parse_wire_values(args);
        let result = self.dispatch(&instance, &function, args);
        match result {
            Ok(value) => {
                if let Some(symbol) = assigned_symbol {
                    self.symbols.insert(symbol, value.clone());
                }
                instruction_result_for_value(id, value)
            }
            Err(ExecuteMethodError::MethodNotFound { class, .. }) => exception(
                id,
                ExecuteMethodError::MethodNotFound {
                    method: protocol_function,
                    class,
                }
                .to_string(),
            ),
            Err(ExecuteMethodError::Control(control)) => control_exception(id, control),
            Err(error) => exception(id, error.to_string()),
        }
    }

    fn dispatch(
        &mut self,
        instance_name: &str,
        method: &str,
        args: Vec<SlimValue>,
    ) -> Result<SlimValue, ExecuteMethodError> {
        let instance = self.instances.get(instance_name).cloned();
        let direct = if let Some(instance) = &instance {
            let result = invoke(instance, method, args.clone());
            if !is_missing_method(&result) {
                return result;
            }
            result
        } else if instance_name == "SlimHelperLibrary" {
            if let Some(result) = self.helper_call(method, args.clone()) {
                if !is_missing_method(&result) {
                    return result;
                }
            }
            Err(ExecuteMethodError::MethodNotFound {
                method: method.into(),
                class: "SlimHelperLibrary".into(),
            })
        } else {
            Err(ExecuteMethodError::ExecutionError(format!(
                "NO_INSTANCE {instance_name}"
            )))
        };
        if let Some(instance) = instance {
            if let Some(fixture) = instance.as_fixture() {
                let sut = fixture
                    .borrow_mut()
                    .execute_system_under_test(method, args.clone());
                if !is_missing_method(&sut) {
                    return sut;
                }
            }
        }
        for (_, library) in self.libraries.iter().rev() {
            let result = invoke(library, method, args.clone());
            if !is_missing_method(&result) {
                return result;
            }
        }
        self.helper_call(method, args).unwrap_or(direct)
    }

    fn helper_call(
        &mut self,
        method: &str,
        args: Vec<SlimValue>,
    ) -> Option<Result<SlimValue, ExecuteMethodError>> {
        let result = match method {
            "get_fixture" if args.is_empty() => self.actor_fixture().map(SlimValue::Object),
            "push_fixture" if args.is_empty() => self.actor_fixture().map(|fixture| {
                self.actors.push(fixture);
                SlimValue::Void
            }),
            "pop_fixture" if args.is_empty() => self
                .actors
                .pop()
                .ok_or_else(|| {
                    ExecuteMethodError::ExecutionError(
                        "ACTOR_STACK_EMPTY message:<<actor stack is empty>>".into(),
                    )
                })
                .map(|fixture| {
                    self.instances.insert("scriptTableActor".into(), fixture);
                    SlimValue::Void
                }),
            "clone_symbol" if args.len() == 1 => {
                Ok(args.into_iter().next().expect("one argument checked"))
            }
            _ => return None,
        };
        Some(result)
    }

    fn actor_fixture(&self) -> Result<SlimObject, ExecuteMethodError> {
        self.instances
            .get("scriptTableActor")
            .cloned()
            .ok_or_else(|| {
                ExecuteMethodError::ExecutionError("NO_INSTANCE scriptTableActor".into())
            })
    }

    fn find_fixture(&self, class: &str) -> Option<&SlimClosureConstructor> {
        self.fixtures.get(class).or_else(|| {
            self.imports
                .iter()
                .rev()
                .find_map(|path| self.fixtures.get(&format!("{path}.{class}")))
        })
    }

    fn parse_wire_values(&self, args: Vec<WireSlimValue>) -> Vec<SlimValue> {
        args.into_iter()
            .map(|value| self.parse_wire_value(value))
            .collect()
    }

    fn parse_wire_value(&self, value: WireSlimValue) -> SlimValue {
        match value {
            WireSlimValue::String(value) if value == "null" => SlimValue::Null,
            WireSlimValue::String(value) => self
                .whole_symbol(&value)
                .unwrap_or_else(|| SlimValue::String(self.replace_symbols(&value))),
            WireSlimValue::List(values) => SlimValue::List(
                values
                    .into_iter()
                    .map(|value| self.parse_wire_value(value))
                    .collect(),
            ),
        }
    }

    fn whole_symbol(&self, value: &str) -> Option<SlimValue> {
        value
            .strip_prefix('$')
            .filter(|name| is_symbol_name(name))
            .and_then(|name| self.symbols.get(name).cloned())
    }

    fn replace_symbols(&self, value: &str) -> String {
        let mut result = String::with_capacity(value.len());
        let mut characters = value.char_indices().peekable();
        while let Some((_, character)) = characters.next() {
            if character != '$' {
                result.push(character);
                continue;
            }
            let start = characters.peek().map_or(value.len(), |(index, _)| *index);
            let mut end = start;
            while let Some((index, character)) = characters.peek().copied() {
                if !character.is_ascii_alphabetic() {
                    break;
                }
                end = index + character.len_utf8();
                characters.next();
            }
            if end == start {
                result.push('$');
            } else {
                let name = &value[start..end];
                match self.symbols.get(name) {
                    Some(symbol) => result.push_str(&symbol.as_text()),
                    None => {
                        result.push('$');
                        result.push_str(name);
                    }
                }
            }
        }
        result
    }
}

/// A server whose transport is selected from the SliM command-line port.
pub type PortSlimServer = SlimServer<Box<dyn Read + Send>, Box<dyn Write + Send>>;

impl SlimServer<Box<dyn Read + Send>, Box<dyn Write + Send>> {
    /// Parses FitNesse's `[-s seconds] port` arguments and opens the selected
    /// transport. Pass `std::env::args().skip(1)` from a server binary.
    pub fn listen_from_args<I, S>(args: I) -> Result<Self, SlimServerError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let values = args
            .into_iter()
            .map(|value| value.as_ref().to_owned())
            .collect::<Vec<_>>();
        let port_value = values.last().ok_or(SlimServerOptionsError::MissingPort)?;
        let port = port_value
            .parse::<u16>()
            .map_err(|_| SlimServerOptionsError::InvalidPort(port_value.clone()))?;
        let options = SlimServerOptions::from_args(&values)?;
        Self::listen_on_port(port, options)
    }

    /// Creates a V0.5 server transport for the supplied SliM port.
    ///
    /// Port `1` uses the current process stdin/stdout. Any other port binds a
    /// TCP listener, accepts one FitNesse connection, then returns a server
    /// ready for fixture registration and [`SlimServer::run`].
    pub fn listen_on_port(port: u16, options: SlimServerOptions) -> Result<Self, SlimServerError> {
        if port == 1 {
            return Ok(Self::with_options(
                Box::new(std::io::stdin()),
                Box::new(std::io::stdout()),
                options,
            ));
        }
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        let (stream, _) = listener.accept()?;
        Ok(Self::with_options(
            Box::new(stream.try_clone()?),
            Box::new(stream),
            options,
        ))
    }
}

/// A line-oriented output tunnel for fixtures running in stdio mode.
///
/// SliM reserves stdout for protocol frames when port `1` is used. Stable
/// Rust cannot safely redirect arbitrary process-wide stdout/stderr without
/// platform-specific file-descriptor manipulation, so fixtures that produce
/// output should write to this explicit adapter, targeting stderr. It emits
/// the V0.5 `SOUT :`/`SOUT.:` or `SERR :`/`SERR.:` prefixes exactly.
pub struct OutputTunnel<W: Write> {
    destination: W,
    first_line: bool,
    at_line_start: bool,
    stream: OutputStream,
}

#[derive(Clone, Copy)]
enum OutputStream {
    Stdout,
    Stderr,
}

impl<W: Write> OutputTunnel<W> {
    pub fn stdout(destination: W) -> Self {
        Self {
            destination,
            first_line: true,
            at_line_start: true,
            stream: OutputStream::Stdout,
        }
    }

    pub fn stderr(destination: W) -> Self {
        Self {
            destination,
            first_line: true,
            at_line_start: true,
            stream: OutputStream::Stderr,
        }
    }

    pub fn into_inner(self) -> W {
        self.destination
    }

    fn prefix(&self) -> &'static [u8] {
        match (self.stream, self.first_line) {
            (OutputStream::Stdout, true) => b"SOUT :",
            (OutputStream::Stdout, false) => b"SOUT.:",
            (OutputStream::Stderr, true) => b"SERR :",
            (OutputStream::Stderr, false) => b"SERR.:",
        }
    }
}

impl<W: Write> Write for OutputTunnel<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        for byte in buffer {
            if self.at_line_start {
                self.destination.write_all(self.prefix())?;
                self.at_line_start = false;
                self.first_line = false;
            }
            self.destination.write_all(std::slice::from_ref(byte))?;
            if *byte == b'\n' {
                self.at_line_start = true;
            }
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.destination.flush()?;
        // A flush after a completed line delimits one fixture log message.
        // Do not reset an unfinished line: doing so would inject a prefix in
        // the middle of its text on the next partial write.
        if self.at_line_start {
            self.first_line = true;
        }
        Ok(())
    }
}

fn invoke(
    object: &SlimObject,
    method: &str,
    args: Vec<SlimValue>,
) -> Result<SlimValue, ExecuteMethodError> {
    let Some(fixture) = object.as_fixture() else {
        return Err(ExecuteMethodError::MethodNotFound {
            method: method.into(),
            class: object.display_value().into(),
        });
    };
    let result = fixture.borrow_mut().execute_method(method, args);
    result
}

fn is_missing_method(result: &Result<SlimValue, ExecuteMethodError>) -> bool {
    matches!(result, Err(ExecuteMethodError::MethodNotFound { .. }))
}

fn is_symbol_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn exception(id: Id, value: String) -> InstructionResult {
    InstructionResult::exception(id, ExceptionMessage::new(value))
}

fn control_exception(id: Id, control: SlimControlException) -> InstructionResult {
    let tag = match control.control {
        SlimControl::AbortSlimTest => "ABORT_SLIM_TEST",
        SlimControl::AbortSlimSuite => "ABORT_SLIM_SUITE",
        SlimControl::IgnoreScriptTest => "IGNORE_SCRIPT_TEST",
        SlimControl::IgnoreAllTests => "IGNORE_ALL_TESTS",
    };
    let message = control
        .message
        .map(|message| format!("message:<<{message}>>"))
        .unwrap_or_default();
    exception(id, format!("{tag}:{message}"))
}

fn control_from_result(result: &InstructionResult) -> Option<SlimControl> {
    let InstructionResultValue::Exception(message) = &result.value else {
        return None;
    };
    let message = message.raw_message();
    [
        ("ABORT_SLIM_TEST:", SlimControl::AbortSlimTest),
        ("ABORT_SLIM_SUITE:", SlimControl::AbortSlimSuite),
        ("IGNORE_SCRIPT_TEST:", SlimControl::IgnoreScriptTest),
        ("IGNORE_ALL_TESTS:", SlimControl::IgnoreAllTests),
    ]
    .into_iter()
    .find_map(|(tag, control)| message.starts_with(tag).then_some(control))
}

fn malformed_instruction(id: Id, fields: Vec<String>) -> InstructionResult {
    exception(id, format!("MALFORMED_INSTRUCTION [{}]", fields.join(",")))
}

fn wire_value_text(value: &WireSlimValue) -> String {
    match value {
        WireSlimValue::String(value) => value.clone(),
        WireSlimValue::List(values) => format!(
            "[{}]",
            values
                .iter()
                .map(wire_value_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn runtime_value_without_symbols(value: WireSlimValue) -> SlimValue {
    match value {
        WireSlimValue::String(value) if value == "null" => SlimValue::Null,
        WireSlimValue::String(value) => SlimValue::String(value),
        WireSlimValue::List(values) => SlimValue::List(
            values
                .into_iter()
                .map(runtime_value_without_symbols)
                .collect(),
        ),
    }
}

fn instruction_result_for_value(id: Id, value: SlimValue) -> InstructionResult {
    match value {
        SlimValue::Void => InstructionResult::void(id),
        value => InstructionResult::new(id, instruction_result_value(value)),
    }
}

fn instruction_result_value(value: SlimValue) -> InstructionResultValue {
    match value {
        SlimValue::String(value) => InstructionResultValue::String(value),
        SlimValue::List(values) => {
            InstructionResultValue::List(values.into_iter().map(instruction_result_value).collect())
        }
        SlimValue::Null => InstructionResultValue::String("null".into()),
        SlimValue::Void => InstructionResultValue::Void,
        SlimValue::Object(value) => InstructionResultValue::String(value.display_value().into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Cursor, thread, time::Duration};

    struct Fixture {
        value: String,
        sut: Sut,
    }
    struct Sut;
    struct Library {
        value: &'static str,
    }
    struct Actor(String);
    struct FailingFixture;

    impl SlimFixture for Fixture {
        fn execute_method(
            &mut self,
            method: &str,
            args: Vec<SlimValue>,
        ) -> Result<SlimValue, ExecuteMethodError> {
            match method {
                "echo" => Ok(args
                    .into_iter()
                    .next()
                    .unwrap_or(SlimValue::String(self.value.clone()))),
                "object" => Ok(SlimValue::Object(SlimObject::fixture(Sut, "chained"))),
                "actor" => {
                    let label = args
                        .into_iter()
                        .next()
                        .map_or_else(|| "actor".into(), |value| value.as_text());
                    Ok(SlimValue::Object(SlimObject::fixture(
                        Actor(label.clone()),
                        label,
                    )))
                }
                _ => Err(ExecuteMethodError::MethodNotFound {
                    method: method.into(),
                    class: "Fixture".into(),
                }),
            }
        }
        fn execute_system_under_test(
            &mut self,
            method: &str,
            args: Vec<SlimValue>,
        ) -> Result<SlimValue, ExecuteMethodError> {
            self.sut.execute_method(method, args)
        }
    }
    impl SlimFixture for Sut {
        fn execute_method(
            &mut self,
            method: &str,
            _args: Vec<SlimValue>,
        ) -> Result<SlimValue, ExecuteMethodError> {
            match method {
                "sut" | "from_sut" | "shared" => Ok(SlimValue::String("sut".into())),
                "echo" => Ok(SlimValue::String("sut-echo".into())),
                _ => Err(ExecuteMethodError::MethodNotFound {
                    method: method.into(),
                    class: "Sut".into(),
                }),
            }
        }
    }
    impl SlimFixture for Library {
        fn execute_method(
            &mut self,
            method: &str,
            _args: Vec<SlimValue>,
        ) -> Result<SlimValue, ExecuteMethodError> {
            if matches!(method, "library" | "shared") {
                Ok(SlimValue::String(self.value.into()))
            } else {
                Err(ExecuteMethodError::MethodNotFound {
                    method: method.into(),
                    class: "Library".into(),
                })
            }
        }
    }
    impl SlimFixture for Actor {
        fn execute_method(
            &mut self,
            method: &str,
            _args: Vec<SlimValue>,
        ) -> Result<SlimValue, ExecuteMethodError> {
            match method {
                "read" => Ok(SlimValue::String(self.0.clone())),
                _ => Err(ExecuteMethodError::MethodNotFound {
                    method: method.into(),
                    class: "Actor".into(),
                }),
            }
        }
    }
    impl SlimFixture for FailingFixture {
        fn execute_method(
            &mut self,
            method: &str,
            _args: Vec<SlimValue>,
        ) -> Result<SlimValue, ExecuteMethodError> {
            match method {
                "abort_test" => Err(ExecuteMethodError::Control(SlimControl::abort_slim_test(
                    "stop this test",
                ))),
                "abort_suite" => Err(ExecuteMethodError::Control(SlimControl::abort_slim_suite(
                    "stop this suite",
                ))),
                "ignore_script" => Err(ExecuteMethodError::Control(
                    SlimControl::ignore_script_test("ignore this script"),
                )),
                "ignore_all" => Err(ExecuteMethodError::Control(SlimControl::ignore_all_tests(
                    "ignore this test",
                ))),
                "slow" => {
                    thread::sleep(Duration::from_millis(1_050));
                    Ok(SlimValue::String("finished late".into()))
                }
                "after" => Ok(SlimValue::String("after".into())),
                _ => Err(ExecuteMethodError::MethodNotFound {
                    method: method.into(),
                    class: "FailingFixture".into(),
                }),
            }
        }
    }
    impl ClassPath for Fixture {
        fn class_path() -> String {
            "Fixture".into()
        }
    }
    impl ClassPath for Library {
        fn class_path() -> String {
            "Library".into()
        }
    }
    impl ClassPath for FailingFixture {
        fn class_path() -> String {
            "FailingFixture".into()
        }
    }
    impl Constructor for Fixture {
        fn construct(args: Vec<SlimValue>) -> Result<Self, ConstructorError> {
            if args.len() > 1 {
                return Err(ConstructorError::NoConstructor);
            }
            Ok(Self {
                value: args
                    .into_iter()
                    .next()
                    .map_or_else(|| "fixture".into(), |value| value.as_text()),
                sut: Sut,
            })
        }
    }
    impl Constructor for Library {
        fn construct(args: Vec<SlimValue>) -> Result<Self, ConstructorError> {
            let [SlimValue::String(value)] = args.as_slice() else {
                return Err(ConstructorError::NoConstructor);
            };
            Ok(Self {
                value: if value == "new" { "newest" } else { "oldest" },
            })
        }
    }
    impl Constructor for FailingFixture {
        fn construct(args: Vec<SlimValue>) -> Result<Self, ConstructorError> {
            match args.as_slice() {
                [] => Ok(Self),
                [SlimValue::String(value)] if value == "conversion" => {
                    Err(ConstructorError::ArgumentParsingError("i64".into()))
                }
                [SlimValue::String(value)] if value == "invocation" => Err(
                    ConstructorError::CouldNotInvoke("constructor failed".into()),
                ),
                [SlimValue::String(value)] if value == "control" => Err(ConstructorError::Control(
                    SlimControl::abort_slim_test("constructor stopped"),
                )),
                _ => Err(ConstructorError::NoConstructor),
            }
        }
    }

    fn server() -> SlimServer<Cursor<Vec<u8>>, Cursor<Vec<u8>>> {
        let mut server = SlimServer::new(Cursor::new(Vec::new()), Cursor::new(Vec::new()));
        server.add_fixture::<Fixture>();
        server.add_fixture::<Library>();
        server.add_fixture::<FailingFixture>();
        server
    }
    fn call(id: &str, instance: &str, function: &str, args: Vec<WireSlimValue>) -> Instruction {
        Instruction::Call {
            id: id.into(),
            instance: instance.into(),
            function: function.into(),
            args,
        }
    }

    #[test]
    fn symbols_are_letter_only_and_preserve_undefined_punctuation_and_values() {
        let mut server = server();
        server
            .symbols
            .insert("A".into(), SlimValue::String("one".into()));
        server
            .symbols
            .insert("B".into(), SlimValue::String("two".into()));
        assert_eq!(
            "$missing.one-two$1",
            server.replace_symbols("$missing.$A-$B$1")
        );
        assert!(
            matches!(server.parse_wire_value("$A".into()), SlimValue::String(value) if value == "one")
        );
        assert_eq!("$not_valid", server.replace_symbols("$not_valid"));
    }

    #[test]
    fn dispatches_fixture_then_sut_then_newest_library_and_helpers() {
        let mut server = server();
        let result = server.execute_instructions(vec![
            Instruction::Make {
                id: "one".into(),
                instance: "fixture".into(),
                class: "Fixture".into(),
                args: vec![],
            },
            Instruction::Make {
                id: "two".into(),
                instance: "libraryOne".into(),
                class: "Library".into(),
                args: vec!["old".into()],
            },
            Instruction::Make {
                id: "three".into(),
                instance: "libraryTwo".into(),
                class: "Library".into(),
                args: vec!["new".into()],
            },
            call("four", "fixture", "fromSut", vec![]),
            call("five", "fixture", "library", vec![]),
            call("six", "fixture", "echo", vec![]),
            call("seven", "fixture", "shared", vec![]),
        ]);
        assert_eq!(
            InstructionResult::string("four".into(), "sut".into()),
            result[3]
        );
        assert_eq!(
            InstructionResult::string("five".into(), "newest".into()),
            result[4]
        );
        assert_eq!(
            InstructionResult::string("six".into(), "fixture".into()),
            result[5]
        );
        assert_eq!(
            InstructionResult::string("seven".into(), "sut".into()),
            result[6]
        );
        assert!(
            matches!(server.dispatch("fixture", "get_fixture", vec![]), Err(ExecuteMethodError::ExecutionError(value)) if value == "NO_INSTANCE scriptTableActor")
        );
    }

    #[test]
    fn call_and_assign_preserves_objects_for_fixture_chaining_and_actors() {
        let mut server = server();
        let result = server.execute_instructions(vec![
            Instruction::Make {
                id: "make".into(),
                instance: "fixture".into(),
                class: "Fixture".into(),
                args: vec![],
            },
            Instruction::CallAndAssign {
                id: "assign".into(),
                symbol: "Actor".into(),
                instance: "fixture".into(),
                function: "object".into(),
                args: vec![],
            },
            Instruction::Make {
                id: "copy".into(),
                instance: "scriptTableActor".into(),
                class: "$Actor".into(),
                args: vec!["ignored".into()],
            },
            call("push", "fixture", "pushFixture", vec![]),
            call("pop", "fixture", "popFixture", vec![]),
            call("chained", "scriptTableActor", "sut", vec![]),
        ]);
        assert!(matches!(
            server.symbols.get("Actor"),
            Some(SlimValue::Object(_))
        ));
        assert_eq!(
            InstructionResult::string("chained".into(), "sut".into()),
            result[5]
        );
    }

    #[test]
    fn make_copies_opaque_object_symbols_without_calling_a_constructor() {
        let mut server = server();
        let object = SlimObject::new(42_i64, "opaque");
        server
            .symbols
            .insert("Opaque".into(), SlimValue::Object(object.clone()));

        let result = server.execute_instructions(vec![Instruction::Make {
            id: "copy".into(),
            instance: "copied".into(),
            class: "$Opaque".into(),
            args: vec!["ignored".into()],
        }]);

        assert_eq!(InstructionResult::ok("copy".into()), result[0]);
        assert_eq!(
            SlimValue::Object(object),
            SlimValue::Object(server.instances.get("copied").unwrap().clone())
        );
    }

    #[test]
    fn actor_helpers_restore_fixture_identity_and_clone_object_symbols() {
        let mut server = server();
        let result = server.execute_instructions(vec![
            Instruction::Make {
                id: "fixture".into(),
                instance: "fixture".into(),
                class: "Fixture".into(),
                args: vec![],
            },
            Instruction::CallAndAssign {
                id: "first-object".into(),
                symbol: "First".into(),
                instance: "fixture".into(),
                function: "actor".into(),
                args: vec!["first".into()],
            },
            Instruction::Make {
                id: "first-actor".into(),
                instance: "scriptTableActor".into(),
                class: "$First".into(),
                args: vec!["ignored".into()],
            },
            call("push", "fixture", "pushFixture", vec![]),
            Instruction::CallAndAssign {
                id: "second-object".into(),
                symbol: "Second".into(),
                instance: "fixture".into(),
                function: "actor".into(),
                args: vec!["second".into()],
            },
            Instruction::Make {
                id: "second-actor".into(),
                instance: "scriptTableActor".into(),
                class: "$Second".into(),
                args: vec![],
            },
            call("second", "scriptTableActor", "read", vec![]),
            call("pop", "fixture", "popFixture", vec![]),
            call("first", "scriptTableActor", "read", vec![]),
            Instruction::CallAndAssign {
                id: "clone".into(),
                symbol: "Clone".into(),
                instance: "fixture".into(),
                function: "cloneSymbol".into(),
                args: vec!["$First".into()],
            },
            Instruction::Make {
                id: "copy".into(),
                instance: "copiedActor".into(),
                class: "$Clone".into(),
                args: vec![],
            },
            call("copied", "copiedActor", "read", vec![]),
        ]);

        assert_eq!(
            InstructionResult::string("second".into(), "second".into()),
            result[6]
        );
        assert_eq!(
            InstructionResult::string("first".into(), "first".into()),
            result[8]
        );
        assert_eq!(
            InstructionResult::string("copied".into(), "first".into()),
            result[11]
        );
    }

    #[test]
    fn pop_fixture_reports_an_empty_actor_stack_separately() {
        let mut server = server();
        let result = server.execute_instructions(vec![
            Instruction::Make {
                id: "fixture".into(),
                instance: "scriptTableActor".into(),
                class: "Fixture".into(),
                args: vec![],
            },
            call("pop", "scriptTableActor", "popFixture", vec![]),
        ]);

        assert_eq!(
            InstructionResult::exception(
                "pop".into(),
                ExceptionMessage::new("ACTOR_STACK_EMPTY message:<<actor stack is empty>>".into())
            ),
            result[1]
        );
    }

    #[test]
    fn reports_standard_constructor_and_lookup_errors() {
        let mut server = server();
        let result = server.execute_instructions(vec![
            Instruction::Make {
                id: "class".into(),
                instance: "x".into(),
                class: "Nope".into(),
                args: vec![],
            },
            Instruction::Make {
                id: "ctor".into(),
                instance: "x".into(),
                class: "Library".into(),
                args: vec![],
            },
            call("instance", "missing", "x", vec![]),
            Instruction::Make {
                id: "conversion".into(),
                instance: "x".into(),
                class: "FailingFixture".into(),
                args: vec!["conversion".into()],
            },
            Instruction::Make {
                id: "invocation".into(),
                instance: "x".into(),
                class: "FailingFixture".into(),
                args: vec!["invocation".into()],
            },
        ]);
        assert_eq!(
            InstructionResult::exception(
                "class".into(),
                ExceptionMessage::new("NO_CLASS Nope".into())
            ),
            result[0]
        );
        assert_eq!(
            InstructionResult::exception(
                "ctor".into(),
                ExceptionMessage::new("NO_CONSTRUCTOR Library".into())
            ),
            result[1]
        );
        assert_eq!(
            InstructionResult::exception(
                "instance".into(),
                ExceptionMessage::new("NO_INSTANCE missing".into())
            ),
            result[2]
        );
        assert_eq!(
            InstructionResult::exception(
                "conversion".into(),
                ExceptionMessage::new("NO_CONVERTER_FOR_ARGUMENT_NUMBER i64".into())
            ),
            result[3]
        );
        assert_eq!(
            InstructionResult::exception(
                "invocation".into(),
                ExceptionMessage::new(
                    "COULD_NOT_INVOKE_CONSTRUCTOR FailingFixture message:<<constructor failed>>"
                        .into()
                )
            ),
            result[4]
        );
    }

    #[test]
    fn missing_targets_still_fall_back_to_libraries_and_preserve_method_spelling() {
        let mut server = server();
        let result = server.execute_instructions(vec![
            Instruction::Make {
                id: "fixture".into(),
                instance: "fixture".into(),
                class: "Fixture".into(),
                args: vec![],
            },
            Instruction::Make {
                id: "library".into(),
                instance: "libraryOne".into(),
                class: "Library".into(),
                args: vec!["new".into()],
            },
            call("fallback", "missing", "library", vec![]),
            call("method", "fixture", "missingMethod", vec![]),
            call(
                "helper",
                "SlimHelperLibrary",
                "cloneSymbol",
                vec!["value".into()],
            ),
        ]);

        assert_eq!(
            InstructionResult::string("fallback".into(), "newest".into()),
            result[2]
        );
        assert_eq!(
            InstructionResult::exception(
                "method".into(),
                ExceptionMessage::new("NO_METHOD_IN_CLASS missingMethod Fixture".into())
            ),
            result[3]
        );
        assert_eq!(
            InstructionResult::string("helper".into(), "value".into()),
            result[4]
        );
    }

    #[test]
    fn invalid_assignment_names_are_malformed_without_invoking_the_call() {
        let mut server = server();
        let result = server.execute_instructions(vec![
            Instruction::Make {
                id: "fixture".into(),
                instance: "fixture".into(),
                class: "Fixture".into(),
                args: vec![],
            },
            Instruction::CallAndAssign {
                id: "bad".into(),
                symbol: "Bad1".into(),
                instance: "fixture".into(),
                function: "echo".into(),
                args: vec!["value".into()],
            },
        ]);

        assert_eq!(
            InstructionResult::exception(
                "bad".into(),
                ExceptionMessage::new(
                    "MALFORMED_INSTRUCTION [bad,callAndAssign,Bad1,fixture,echo,value]".into()
                )
            ),
            result[1]
        );
        assert!(!server.symbols.contains_key("Bad1"));
    }

    #[test]
    fn symbol_substitution_preserves_typed_whole_values_and_walks_nested_lists() {
        let mut server = server();
        server
            .symbols
            .insert("Text".into(), SlimValue::String("value".into()));
        server.symbols.insert("Nothing".into(), SlimValue::Null);
        server.symbols.insert(
            "Items".into(),
            SlimValue::List(vec![SlimValue::String("one".into())]),
        );

        assert_eq!(SlimValue::Null, server.parse_wire_value("$Nothing".into()));
        assert_eq!(
            SlimValue::List(vec![SlimValue::String("one".into())]),
            server.parse_wire_value("$Items".into())
        );
        assert_eq!(
            SlimValue::List(vec![
                SlimValue::String("value".into()),
                SlimValue::List(vec![SlimValue::Null, SlimValue::String("$Missing!".into()),]),
            ]),
            server.parse_wire_value(WireSlimValue::List(vec![
                "$Text".into(),
                WireSlimValue::List(vec!["$Nothing".into(), "$Missing!".into()]),
            ]))
        );
        assert_eq!(
            "before null after",
            server.replace_symbols("before $Nothing after")
        );
    }

    #[test]
    fn assign_preserves_literal_symbols_while_retaining_lists_and_null() {
        let mut server = server();
        server
            .symbols
            .insert("Existing".into(), SlimValue::String("expanded".into()));
        let result = server.execute_instructions(vec![
            Instruction::Assign {
                id: "literal".into(),
                symbol: "Literal".into(),
                value: "$Existing".into(),
            },
            Instruction::Assign {
                id: "list".into(),
                symbol: "List".into(),
                value: WireSlimValue::List(vec!["$Existing".into(), "null".into()]),
            },
        ]);

        assert_eq!(InstructionResult::ok("literal".into()), result[0]);
        assert_eq!(
            Some(&SlimValue::String("$Existing".into())),
            server.symbols.get("Literal")
        );
        assert_eq!(
            Some(&SlimValue::List(vec![
                SlimValue::String("$Existing".into()),
                SlimValue::Null,
            ])),
            server.symbols.get("List")
        );
    }

    #[test]
    fn malformed_instructions_keep_the_batch_running() {
        let mut server = server();
        let result = server.execute_instructions(vec![
            Instruction::Malformed {
                id: "bad".into(),
                fields: vec!["bad".into(), "unknown".into()],
            },
            Instruction::Import {
                id: "good".into(),
                path: "Fixtures".into(),
            },
        ]);

        assert_eq!(
            InstructionResult::exception(
                "bad".into(),
                ExceptionMessage::new("MALFORMED_INSTRUCTION [bad,unknown]".into())
            ),
            result[0]
        );
        assert_eq!(InstructionResult::ok("good".into()), result[1]);
        assert_eq!(vec!["Fixtures"], server.imports);
    }

    #[test]
    fn exact_class_names_win_and_the_newest_import_is_searched_first() {
        let mut server = server();
        for (class, value) in [("First.Thing", "oldest"), ("Second.Thing", "newest")] {
            server.fixtures.insert(
                class.into(),
                Box::new(move |_| Ok(SlimObject::fixture(Library { value }, class))),
            );
        }
        let result = server.execute_instructions(vec![
            Instruction::Import {
                id: "first-import".into(),
                path: "First".into(),
            },
            Instruction::Import {
                id: "second-import".into(),
                path: "Second".into(),
            },
            Instruction::Make {
                id: "short".into(),
                instance: "short".into(),
                class: "Thing".into(),
                args: vec![],
            },
            call("short-call", "short", "library", vec![]),
            Instruction::Make {
                id: "exact".into(),
                instance: "exact".into(),
                class: "First.Thing".into(),
                args: vec![],
            },
            call("exact-call", "exact", "library", vec![]),
        ]);

        assert_eq!(
            InstructionResult::string("short-call".into(), "newest".into()),
            result[3]
        );
        assert_eq!(
            InstructionResult::string("exact-call".into(), "oldest".into()),
            result[5]
        );
    }

    #[test]
    fn control_exceptions_have_exact_tags_and_stop_only_the_current_batch() {
        let controls = [
            ("abort_test", "ABORT_SLIM_TEST:message:<<stop this test>>"),
            (
                "abort_suite",
                "ABORT_SLIM_SUITE:message:<<stop this suite>>",
            ),
            (
                "ignore_script",
                "IGNORE_SCRIPT_TEST:message:<<ignore this script>>",
            ),
            (
                "ignore_all",
                "IGNORE_ALL_TESTS:message:<<ignore this test>>",
            ),
        ];

        for (method, expected) in controls {
            let mut server = server();
            assert_eq!(
                vec![InstructionResult::ok("make".into())],
                server.execute_instructions(vec![Instruction::Make {
                    id: "make".into(),
                    instance: "failing".into(),
                    class: "FailingFixture".into(),
                    args: vec![],
                }])
            );
            let batch = server.execute_instructions(vec![
                call("control", "failing", method, vec![]),
                call("skipped", "failing", "after", vec![]),
            ]);
            assert_eq!(1, batch.len());
            assert_eq!(
                InstructionResult::exception(
                    "control".into(),
                    ExceptionMessage::new(expected.into())
                ),
                batch[0]
            );
            assert_eq!(
                vec![InstructionResult::string("next".into(), "after".into())],
                server.execute_instructions(vec![call("next", "failing", "after", vec![])])
            );
        }

        let mut server = server();
        let result = server.execute_instructions(vec![
            Instruction::Make {
                id: "constructor-control".into(),
                instance: "never-created".into(),
                class: "FailingFixture".into(),
                args: vec!["control".into()],
            },
            Instruction::Import {
                id: "skipped".into(),
                path: "Skipped".into(),
            },
        ]);
        assert_eq!(
            vec![InstructionResult::exception(
                "constructor-control".into(),
                ExceptionMessage::new("ABORT_SLIM_TEST:message:<<constructor stopped>>".into()),
            )],
            result
        );
        assert!(server.imports.is_empty());
        assert_eq!(
            InstructionResult::exception(
                "plain".into(),
                ExceptionMessage::new("IGNORE_ALL_TESTS:".into())
            ),
            control_exception(
                "plain".into(),
                SlimControlException::without_message(SlimControl::IgnoreAllTests)
            )
        );
    }

    #[test]
    fn soft_timeout_reports_the_protocol_message_and_preserves_side_effects() {
        let mut server = SlimServer::with_options(
            Cursor::new(Vec::new()),
            Cursor::new(Vec::new()),
            SlimServerOptions::with_instruction_timeout_seconds(0),
        );
        server.instances.insert(
            "failing".into(),
            SlimObject::fixture(FailingFixture, "FailingFixture"),
        );
        let result = server.execute_instructions(vec![
            Instruction::Import {
                id: "import".into(),
                path: "Fixtures".into(),
            },
            Instruction::Assign {
                id: "assign".into(),
                symbol: "Value".into(),
                value: "value".into(),
            },
            call("call", "failing", "after", vec![]),
            Instruction::CallAndAssign {
                id: "call-and-assign".into(),
                symbol: "Late".into(),
                instance: "failing".into(),
                function: "after".into(),
                args: vec![],
            },
        ]);
        assert_eq!(InstructionResult::ok("import".into()), result[0]);
        assert_eq!(InstructionResult::ok("assign".into()), result[1]);
        assert_eq!(
            InstructionResult::exception(
                "call".into(),
                ExceptionMessage::new("TIMED_OUT 0".into()),
            ),
            result[2]
        );
        assert_eq!(
            InstructionResult::exception(
                "call-and-assign".into(),
                ExceptionMessage::new("TIMED_OUT 0".into()),
            ),
            result[3]
        );
        assert_eq!(
            Some(&SlimValue::String("value".into())),
            server.symbols.get("Value")
        );
        assert_eq!(
            Some(&SlimValue::String("after".into())),
            server.symbols.get("Late")
        );

        let control = server.execute_instructions(vec![
            call("control", "failing", "abort_test", vec![]),
            call("skipped", "failing", "after", vec![]),
        ]);
        assert_eq!(1, control.len());
        assert_eq!(
            InstructionResult::exception(
                "control".into(),
                ExceptionMessage::new("ABORT_SLIM_TEST:message:<<stop this test>>".into()),
            ),
            control[0]
        );
    }

    #[test]
    fn parses_documented_timeout_flags() {
        assert_eq!(
            SlimServerOptions::with_instruction_timeout_seconds(7),
            SlimServerOptions::from_slim_flags("-v -s 7").unwrap()
        );
        assert_eq!(
            SlimServerOptions::with_instruction_timeout_seconds(3),
            SlimServerOptions::from_slim_flags("-s3").unwrap()
        );
        assert_eq!(
            Err(SlimServerOptionsError::MissingTimeout),
            SlimServerOptions::from_slim_flags("-s")
        );

        let server = PortSlimServer::listen_from_args(["-s", "9", "1"]).unwrap();
        assert_eq!(
            SlimServerOptions::with_instruction_timeout_seconds(9),
            server.options
        );
        assert!(matches!(
            PortSlimServer::listen_from_args(Vec::<String>::new()),
            Err(SlimServerError::Options(
                SlimServerOptionsError::MissingPort
            ))
        ));
        assert!(matches!(
            PortSlimServer::listen_from_args(["not-a-port"]),
            Err(SlimServerError::Options(
                SlimServerOptionsError::InvalidPort(value)
            )) if value == "not-a-port"
        ));
    }

    #[test]
    fn output_tunnel_uses_v05_prefixes_for_each_line() {
        let mut stdout = OutputTunnel::stdout(Vec::new());
        stdout.write_all(b"one\ntwo").unwrap();
        stdout.write_all(b"\nthree\n").unwrap();
        stdout.flush().unwrap();
        stdout.write_all(b"again\n").unwrap();
        assert_eq!(
            b"SOUT :one\nSOUT.:two\nSOUT.:three\nSOUT :again\n",
            stdout.into_inner().as_slice()
        );

        let mut stderr = OutputTunnel::stderr(Vec::new());
        stderr.write_all(b"problem\nagain").unwrap();
        stderr.flush().unwrap();
        stderr.write_all(b" still here").unwrap();
        assert_eq!(
            b"SERR :problem\nSERR.:again still here",
            stderr.into_inner().as_slice()
        );
    }

    #[test]
    fn abort_suite_does_not_close_the_connection_for_the_next_batch() {
        let batches = [
            vec![Instruction::Make {
                id: "make".into(),
                instance: "failing".into(),
                class: "FailingFixture".into(),
                args: vec![],
            }],
            vec![
                call("abort", "failing", "abort_suite", vec![]),
                call("skipped", "failing", "after", vec![]),
            ],
            vec![call("next", "failing", "after", vec![])],
        ];
        let mut input = batches
            .iter()
            .flat_map(|batch| batch.to_slim_string().as_bytes().to_vec())
            .collect::<Vec<_>>();
        input.extend_from_slice("bye".to_slim_string().as_bytes());
        let mut output = Vec::new();
        let mut server = SlimServer::new(Cursor::new(input), Cursor::new(&mut output));
        server.add_fixture::<FailingFixture>();
        server.run().unwrap();

        let expected_batches = [
            vec![InstructionResult::ok("make".into())],
            vec![InstructionResult::exception(
                "abort".into(),
                ExceptionMessage::new("ABORT_SLIM_SUITE:message:<<stop this suite>>".into()),
            )],
            vec![InstructionResult::string("next".into(), "after".into())],
        ];
        let mut expected = b"Slim -- V0.5\n".to_vec();
        for batch in expected_batches {
            expected.extend_from_slice(batch.to_slim_string().as_bytes());
        }
        assert_eq!(expected, output);
    }

    #[test]
    fn run_writes_a_handshake_response_and_stops_at_bye() {
        let mut input = vec![Instruction::Import {
            id: "import".into(),
            path: "Fixtures".into(),
        }]
        .to_slim_string()
        .as_bytes()
        .to_vec();
        input.extend_from_slice("bye".to_slim_string().as_bytes());
        let mut output = Vec::new();

        SlimServer::new(Cursor::new(input), Cursor::new(&mut output))
            .run()
            .unwrap();

        let mut expected = b"Slim -- V0.5\n".to_vec();
        expected.extend_from_slice(
            vec![InstructionResult::ok("import".into())]
                .to_slim_string()
                .as_bytes(),
        );
        assert_eq!(expected, output);
    }
}
