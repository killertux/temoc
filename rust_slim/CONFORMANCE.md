# SliM V0.5 conformance matrix

This matrix tracks the documented [SliM V0.5 protocol](https://fitnesse.org/FitNesse/UserGuide/WritingAcceptanceTests/SliM/SlimProtocol.html). “Covered” means the workspace has a unit or child-process TCP/stdio test for the behavior.

| Area | V0.5 status | Coverage and notes |
| --- | --- | --- |
| Framing, V0.5 handshake, `bye` | Supported | Covered; outer frames use UTF-8 byte lengths and inner strings use UTF-16 units. |
| Recursive strings and lists | Supported | Covered, including accented and surrogate-pair Unicode. |
| `import`, `make`, `call`, `callAndAssign`, `assign` | Supported | Covered through TCP end-to-end batches. |
| Constructors and conversions | Supported | Arity, conversion, and fixture failure errors are covered. |
| String, null, list, and object symbols | Supported | Letter-only V0.5 symbol names; recursive lists and object fixture chaining are covered. Current-reference extensions such as digits, underscores, and backticks are intentionally excluded. |
| SUT, library stack, actor helper, `cloneSymbol` | Supported | Fixture → SUT → newest library dispatch and actor push/pop are covered. |
| Standard exceptions | Supported | Standard error tokens are placed in `message:<<…>>` envelopes for FitNesse display compatibility. Control tags retain their required raw prefixes. |
| Stop/ignore controls | Supported | Stop and ignore end only the current batch; the connection remains usable. |
| Timeout | Supported with documented limit | `-s` is observational after a fixture returns. It cannot forcibly cancel arbitrary in-process Rust code. |
| TCP and port `1` stdio | Supported | Child-process tests cover both transports and repeated batches on one connection. |
| Output tunneling | Supported through explicit adapter | `OutputTunnel` writes V0.5 prefixes to stderr. Arbitrary process-global `println!`/stderr interception is intentionally not attempted. |
| Dates and collections | Supported | Dates use English `dd-MMM-yyyy` abbreviations. Collections accept recursive wire lists and the documented bracketed string form. |
| HTML hash widget | Optional, supported behind `html-hash` | Uses the opt-in `SlimHash` converter. Invalid/multiple tables yield an empty map; malformed rows are ignored. |
| Live FitNesse suite | Not yet automated | The repository has protocol-level TCP/stdio conformance coverage, but does not yet run an external FitNesse suite in CI. |
