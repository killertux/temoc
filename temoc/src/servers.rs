//! The fixture servers of a run. A table runs on the server that the last
//! `server` directive above it names, or on the default one.

use crate::config::{config_bool, config_str, config_u16};
use anyhow::{anyhow, bail, Result};
use toml::Table;

/// What the only server is called when the configuration doesn't name it.
pub const UNNAMED_SERVER: &str = "default";

/// The settings that belong to a server and not to the run.
const SERVER_KEYS: [&str; 6] = [
    "execute_server_command",
    "port",
    "pool_size",
    "persistent_server",
    "file_hooks",
    "pipe_output",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub name: String,
    pub command: String,
    pub port: u16,
    pub pool_size: u16,
    pub pipe_output: bool,
    /// Keeps the server for the whole run, instead of one for each file.
    pub persistent: bool,
    /// The fixture whose `beginFile` and `endFile` surround every file that
    /// uses this server.
    pub file_hooks: Option<String>,
}

impl ServerConfig {
    /// How messages about a server that can't be used call it.
    pub fn describe(&self) -> String {
        format!("Server `{}` (`{}`)", self.name, self.command)
    }
}

/// What the command line sets. It applies to the default server.
#[derive(Debug, Default)]
pub struct Overrides {
    pub command: Option<String>,
    pub port: Option<u16>,
    pub pool_size: Option<u16>,
    pub persistent: bool,
    pub file_hooks: Option<String>,
    pub pipe_output: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Servers {
    configs: Vec<ServerConfig>,
    default: usize,
    selected: Vec<bool>,
}

impl Servers {
    pub fn from_config(config: Option<&Table>, overrides: Overrides) -> Result<Self> {
        let mut entries = match config {
            Some(config) => read_entries(config)?,
            None => vec![Entry::unnamed()],
        };
        let default = default_entry(&entries)?;
        entries[default].apply(overrides);
        let configs = entries
            .into_iter()
            .map(Entry::finish)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            selected: vec![true; configs.len()],
            configs,
            default,
        })
    }

    /// Limits the run to the tables of the named servers. No name keeps all.
    pub fn select(&mut self, names: &[String]) -> Result<()> {
        let names: Vec<&str> = names
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .collect();
        if names.is_empty() {
            return Ok(());
        }
        let mut selected = vec![false; self.configs.len()];
        for name in names {
            let index = self.index_of(name).ok_or_else(|| {
                anyhow!(
                    "Unknown server `{name}` in --server. The configured servers are: {}",
                    self.names()
                )
            })?;
            selected[index] = true;
        }
        self.selected = selected;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.configs.len()
    }

    pub fn configs(&self) -> &[ServerConfig] {
        &self.configs
    }

    #[cfg(test)]
    pub fn get(&self, index: usize) -> &ServerConfig {
        &self.configs[index]
    }

    #[cfg(test)]
    pub fn name(&self, index: usize) -> &str {
        &self.configs[index].name
    }

    pub fn default_index(&self) -> usize {
        self.default
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected[index]
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.configs.iter().position(|config| config.name == name)
    }

    /// Every name, for messages.
    pub fn names(&self) -> String {
        self.configs
            .iter()
            .map(|config| format!("`{}`", config.name))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// A server as the configuration file has it, before the command line and
/// the defaults fill it in.
#[derive(Debug, Default)]
struct Entry {
    name: String,
    named: bool,
    default: bool,
    command: Option<String>,
    port: Option<u16>,
    pool_size: Option<u16>,
    persistent: bool,
    file_hooks: Option<String>,
    pipe_output: bool,
}

impl Entry {
    fn unnamed() -> Self {
        Self {
            name: UNNAMED_SERVER.to_string(),
            ..Self::default()
        }
    }

    /// Reads the server keys of `table`. `place` says where the table is, to
    /// end the message about a bad value.
    fn read(table: &Table, place: &str) -> Result<Self> {
        let read = || -> Result<Self> {
            Ok(Self {
                port: config_u16(table, "port")?,
                pool_size: config_u16(table, "pool_size")?,
                pipe_output: config_bool(
                    table,
                    "pipe_output",
                    "Expect the pipe_output to be a boolean",
                )?,
                persistent: config_bool(
                    table,
                    "persistent_server",
                    "Expect the persistent_server to be a boolean",
                )?,
                file_hooks: config_str(
                    table,
                    "file_hooks",
                    "Expect the file_hooks to be a string",
                )?,
                command: config_str(
                    table,
                    "execute_server_command",
                    "Expect the slim server command to be a string",
                )?,
                default: config_bool(table, "default", "Expect the default to be a boolean")?,
                ..Self::unnamed()
            })
        };
        read().map_err(|error| anyhow!("{error}{place}"))
    }

    fn apply(&mut self, overrides: Overrides) {
        self.command = overrides.command.or(self.command.take());
        self.port = overrides.port.or(self.port);
        self.pool_size = overrides.pool_size.or(self.pool_size);
        self.file_hooks = overrides.file_hooks.or(self.file_hooks.take());
        self.persistent |= overrides.persistent;
        self.pipe_output |= overrides.pipe_output;
    }

    fn finish(self) -> Result<ServerConfig> {
        let Some(command) = self.command else {
            if self.named {
                bail!(
                    "Server `{}` needs an `execute_server_command`, the command that starts it",
                    self.name
                );
            }
            bail!("You need to provide a command to start the slim server");
        };
        Ok(ServerConfig {
            name: self.name,
            command,
            port: self.port.unwrap_or(8085),
            pool_size: self.pool_size.unwrap_or(20),
            pipe_output: self.pipe_output,
            persistent: self.persistent,
            file_hooks: self.file_hooks,
        })
    }
}

fn read_entries(config: &Table) -> Result<Vec<Entry>> {
    let Some(servers) = config.get("servers") else {
        return Ok(vec![Entry::read(config, "")?]);
    };
    let servers = servers.as_table().ok_or_else(|| {
        anyhow!("Expected `servers` to hold a table for each server, like `[servers.name]`")
    })?;
    if let Some(key) = SERVER_KEYS.iter().find(|key| config.contains_key(**key)) {
        bail!(
            "`{key}` is set at the top level of a configuration that also has `[servers.*]`. Move it into the server it belongs to, like `[servers.name]`"
        );
    }
    if servers.is_empty() {
        bail!("`[servers]` has no server. Add one, like `[servers.name]`");
    }
    servers
        .iter()
        .map(|(name, value)| {
            let table = value.as_table().ok_or_else(|| {
                anyhow!("Expected `servers.{name}` to be a table, like `[servers.{name}]`")
            })?;
            if name.is_empty() || name.contains(|c: char| c.is_whitespace() || c == ',') {
                bail!("The server name `{name}` can't be empty or have spaces or commas");
            }
            if let Some(key) = table
                .keys()
                .find(|key| *key != "default" && !SERVER_KEYS.contains(&key.as_str()))
            {
                bail!(
                    "Unknown key `{key}` in `[servers.{name}]`. A server has: default, {}",
                    SERVER_KEYS.join(", ")
                );
            }
            Ok(Entry {
                name: name.clone(),
                named: true,
                ..Entry::read(table, &format!(" in `[servers.{name}]`"))?
            })
        })
        .collect()
}

/// The server of the pages that have no `server` directive.
fn default_entry(entries: &[Entry]) -> Result<usize> {
    if entries.len() == 1 {
        return Ok(0);
    }
    let names = |entries: Vec<&Entry>| {
        entries
            .iter()
            .map(|entry| format!("`{}`", entry.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let defaults: Vec<&Entry> = entries.iter().filter(|entry| entry.default).collect();
    match defaults.as_slice() {
        [default] => Ok(entries
            .iter()
            .position(|entry| entry.name == default.name)
            .unwrap_or_default()),
        [] => bail!(
            "Several servers are configured ({}) and none is the default. Add `default = true` to the one that pages without a `server` directive use",
            names(entries.iter().collect())
        ),
        _ => bail!(
            "Only one server can be the default, but {} all have `default = true`",
            names(defaults)
        ),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn servers(toml: &str) -> Result<Servers> {
        Servers::from_config(Some(&toml.parse::<Table>().unwrap()), Overrides::default())
    }

    fn error(toml: &str) -> String {
        servers(toml).unwrap_err().to_string()
    }

    const TWO: &str = "
        [servers.server]
        default = true
        execute_server_command = \"a\"
        port = 1
        persistent_server = true
        file_hooks = \"SpecHooks\"

        [servers.machines]
        execute_server_command = \"b\"
    ";

    #[test]
    fn top_level_keys_are_one_unnamed_default_server() {
        let servers = servers("execute_server_command = \"run\"\nport = 1\n").unwrap();
        assert_eq!(1, servers.len());
        assert_eq!(0, servers.default_index());
        assert_eq!(
            &ServerConfig {
                name: "default".into(),
                command: "run".into(),
                port: 1,
                pool_size: 20,
                pipe_output: false,
                persistent: false,
                file_hooks: None,
            },
            servers.get(0)
        );
    }

    #[test]
    fn named_servers_keep_their_own_settings() {
        let servers = servers(TWO).unwrap();
        let default = servers.get(servers.default_index());
        assert_eq!("server", default.name);
        assert_eq!(1, default.port);
        assert!(default.persistent);
        assert_eq!(Some("SpecHooks".to_string()), default.file_hooks);
        let machines = servers.get(servers.index_of("machines").unwrap());
        assert_eq!("b", machines.command);
        assert_eq!(8085, machines.port);
        assert!(!machines.persistent);
        assert_eq!(None, machines.file_hooks);
    }

    #[test]
    fn one_named_server_is_the_default_without_saying_so() {
        let servers = servers("[servers.only]\nexecute_server_command = \"a\"\n").unwrap();
        assert_eq!("only", servers.name(servers.default_index()));
    }

    #[test]
    fn several_servers_need_a_default() {
        assert_eq!(
            "Several servers are configured (`a`, `b`) and none is the default. Add `default = true` to the one that pages without a `server` directive use",
            error("[servers.a]\nexecute_server_command = \"a\"\n[servers.b]\nexecute_server_command = \"b\"\n")
        );
    }

    #[test]
    fn only_one_server_can_be_the_default() {
        assert_eq!(
            "Only one server can be the default, but `a`, `b` all have `default = true`",
            error("[servers.a]\ndefault = true\nexecute_server_command = \"a\"\n[servers.b]\ndefault = true\nexecute_server_command = \"b\"\n")
        );
    }

    #[test]
    fn top_level_server_keys_can_not_be_mixed_with_named_servers() {
        assert_eq!(
            "`port` is set at the top level of a configuration that also has `[servers.*]`. Move it into the server it belongs to, like `[servers.name]`",
            error("port = 1\n[servers.a]\nexecute_server_command = \"a\"\n")
        );
    }

    #[test]
    fn every_named_server_needs_a_command() {
        assert_eq!(
            "Server `b` needs an `execute_server_command`, the command that starts it",
            error("[servers.a]\ndefault = true\nexecute_server_command = \"a\"\n[servers.b]\n")
        );
    }

    #[test]
    fn unknown_server_keys_and_bad_values_say_where_they_are() {
        assert_eq!(
            "Unknown key `prot` in `[servers.a]`. A server has: default, execute_server_command, port, pool_size, persistent_server, file_hooks, pipe_output",
            error("[servers.a]\nprot = 1\n")
        );
        assert_eq!(
            "Expected `port` to be a number in `[servers.a]`",
            error("[servers.a]\nport = \"one\"\n")
        );
    }

    #[test]
    fn the_command_line_sets_the_default_server() {
        let servers = Servers::from_config(
            Some(&TWO.parse::<Table>().unwrap()),
            Overrides {
                command: Some("other".into()),
                port: Some(9000),
                pool_size: Some(3),
                persistent: false,
                file_hooks: Some("Other".into()),
                pipe_output: true,
            },
        )
        .unwrap();
        let default = servers.get(servers.default_index());
        assert_eq!("other", default.command);
        assert_eq!(9000, default.port);
        assert_eq!(3, default.pool_size);
        assert!(default.persistent);
        assert!(default.pipe_output);
        assert_eq!(Some("Other".to_string()), default.file_hooks);
        let machines = servers.get(servers.index_of("machines").unwrap());
        assert_eq!("b", machines.command);
        assert!(!machines.pipe_output);
    }

    #[test]
    fn no_configuration_file_is_one_server_from_the_command_line() {
        let servers = Servers::from_config(
            None,
            Overrides {
                command: Some("run".into()),
                ..Overrides::default()
            },
        )
        .unwrap();
        assert_eq!("default", servers.name(0));
        assert_eq!(
            "You need to provide a command to start the slim server",
            Servers::from_config(None, Overrides::default())
                .unwrap_err()
                .to_string()
        );
    }

    #[test]
    fn selecting_servers_by_name() {
        let mut servers = servers(TWO).unwrap();
        let machines = servers.index_of("machines").unwrap();
        let server = servers.index_of("server").unwrap();
        assert!(servers.is_selected(machines) && servers.is_selected(server));
        servers.select(&["machines".into()]).unwrap();
        assert!(servers.is_selected(machines));
        assert!(!servers.is_selected(server));
        assert_eq!(
            "Unknown server `nosuch` in --server. The configured servers are: `machines`, `server`",
            servers.select(&["nosuch".into()]).unwrap_err().to_string()
        );
    }
}
