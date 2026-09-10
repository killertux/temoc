## Rust Slim - Slim server for Rust

Develop Slim Fixtures for rust applications. Based on the [Slim Protocol](https://fitnesse.org/FitNesse/UserGuide/WritingAcceptanceTests/SliM/SlimProtocol.html) of fitnesse [fitnesse](https://fitnesse.org) .

This is not 100% compliant with the SliM protocol yet. Remaining work includes
instruction timeouts, stop and ignore batch control, standard input/output
transport with fixture-output tunneling, and the optional HTML hash converter.

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
