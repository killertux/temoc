# Changelog

## temoc 0.12.0

One run can now check a tree of pages against several fixture servers.

- Configuration: `[servers.<name>]` tables, each with its own
  `execute_server_command`, `port`, `pool_size`, `persistent_server`,
  `file_hooks` and `pipe_output`. One of them has `default = true`. A
  configuration with those keys at the top level keeps working as a single
  server.
- Pages: the `server <name>` directive selects the server of every table and
  `import` after it in the file. A page without it uses the default server.
- Servers start when a table first needs them, so a server that no table of
  the run uses never starts.
- File hooks are called on each server that a file uses.
- `--server <name>` runs only the tables of the named servers.
- A `$NAME` cell for a symbol that another server set fails with a message
  that names both servers, instead of quietly using the wrong value.
- With several servers, one that fails to start or dies fails the tables that
  need it and the run goes on. The message names the server and its command,
  also when it is the only server.
- JSONL output: a `server` field on the table, row, `setup_failed` and
  `output` events, on each table of `file_started`, and on a `run_error` about
  a server. A new `symbol_from_other_server` kind of `actual`. The schema is
  still 1.
- Text output: in a file that uses several servers, failures end with
  ` on server <name>`. With several servers configured, piped server output
  starts with `[<name>] `.
- An `import` is not sent to a server on which the file runs no table.
