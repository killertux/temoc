## Rust Slim - Slim server for Rust

Develop Slim Fixtures for rust applications. Based on the [Slim Protocol](https://fitnesse.org/FitNesse/UserGuide/WritingAcceptanceTests/SliM/SlimProtocol.html) of fitnesse [fitnesse](https://fitnesse.org) .

See the maintained [V0.5 conformance matrix](CONFORMANCE.md) for supported,
optional, and intentionally excluded behavior. The runtime intentionally
provides an observational timeout instead of isolation-backed hard
cancellation, and explicit output tunnels instead of transparent process-wide
stdio capture; those limitations are described below.

This is currently in an unstable version. The general API can change in the next versions.

For more details, take a look at the [documentation](https://docs.rs/rust_slim/latest/rust_slim/)

## Migrating manually implemented fixtures

`SlimFixture::execute_method` now receives `Vec<SlimValue>` and returns
`Result<SlimValue, ExecuteMethodError>`. Use `FromSlimValue` to convert each
argument and `IntoSlimValue` to convert the method result. These conversions
preserve protocol lists, null, void, and in-process object handles. The old
`ToSlimResultString` trait remains available as a string conversion helper,
but it does not preserve those structured values.

Manual fixtures must also implement `Constructor::construct` and return
`Result<Self, ConstructorError>`. The `#[fixture]` macro generates a zero-argument
constructor for `Default` fixtures. Mark one or more associated functions with
`#[slim(constructor)]` to expose typed constructors of different arities, and
mark an `&mut self` accessor with `#[slim(sut)]` to provide a System Under Test.

## Runtime controls, timeouts, and transports

Fixtures can stop the remainder of the current instruction batch by returning
`ExecuteMethodError::Control`. Use `SlimControl::abort_slim_test`,
`abort_slim_suite`, `ignore_script_test`, or `ignore_all_tests`; each produces
the corresponding V0.5 exception tag and an optional `message:<<...>>` reason.
The next batch on the same connection remains usable.

Create a transport with `PortSlimServer::listen_from_args(std::env::args().skip(1))`.
It parses FitNesse's documented `-s <seconds>` timeout flag followed by the port.
Port `1` uses stdin/stdout; other ports bind `0.0.0.0:port`, accept one TCP
connection, and then run the normal V0.5 handshake. Responses and client
requests are flushed after every frame. Applications that already parse their
arguments can use `listen_on_port(port, options)` directly.

The timeout is observational: the single-threaded server measures elapsed time
after an instruction returns and emits `TIMED_OUT n` for an overrun. Arbitrary
Rust fixture code is not forcibly cancelled, so effects it made before returning
remain. A control exception takes precedence over a simultaneous timeout. This
avoids unsound cross-thread cancellation; fixtures that require prompt
cancellation must expose a cooperative mechanism.

When using port `1`, protocol stdout cannot also carry fixture output. Stable
Rust cannot intercept arbitrary process-wide `println!` or stderr writes
without platform-specific descriptor manipulation. Write intentional fixture
output to `OutputTunnel::stdout(std::io::stderr())` or
`OutputTunnel::stderr(std::io::stderr())`; it emits the V0.5 `SOUT :`/
`SOUT.:` and `SERR :`/`SERR.:` prefixes line by line.

## Values and optional HTML hashes

`Vec<T>` and arrays accept either recursive SliM wire lists or the documented
Java-style textual form such as `[one, two]`; nested textual lists are also
accepted. `NaiveDate` uses the protocol’s English `dd-MMM-yyyy` form (for
example, `10-Oct-1970`), independent of the host locale.

FitNesse’s HTML hash widget is optional in V0.5. Enable it with
`rust_slim = { version = "0.3", features = ["html-hash"] }` and use `SlimHash` as a fixture
argument or return value. It exposes a deterministic `BTreeMap<String,
String>` through `as_map`/`into_inner` and serializes returns as escaped
two-column HTML tables. One valid table is converted; invalid or multiple
tables become an empty map, and malformed rows are ignored.

```rust,ignore
use rust_slim::{fixture, SlimHash};

# struct Fixture;
#[fixture]
impl Fixture {
    pub fn lookup(&self, values: SlimHash) -> String {
        values.as_map().get("name").cloned().unwrap_or_default()
    }
}
```

FitNesse recognizes standard protocol errors when they are enclosed in
`message:<<…>>`. `rust_slim` emits that envelope for standard errors such as
`NO_METHOD_IN_CLASS` and `TIMED_OUT`; abort and ignore tags retain their raw
control prefixes and optional message suffix.
