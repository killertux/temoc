//! Reads typed values out of the configuration file.

use anyhow::{anyhow, Result};
use toml::Table;

pub fn config_u16(config_file: &Table, key: &str) -> Result<Option<u16>> {
    config_file
        .get(key)
        .map(|value| {
            let value = value
                .as_integer()
                .ok_or_else(|| anyhow!("Expected `{key}` to be a number"))?;
            u16::try_from(value)
                .map_err(|_| anyhow!("Expected `{key}` to be between 0 and 65535 but got {value}"))
        })
        .transpose()
}

pub fn config_bool(config_file: &Table, key: &str, message: &str) -> Result<bool> {
    config_file.get(key).map_or(Ok(false), |value| {
        value.as_bool().ok_or_else(|| anyhow!("{message}"))
    })
}

pub fn config_str(config_file: &Table, key: &str, message: &str) -> Result<Option<String>> {
    config_file
        .get(key)
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow!("{message}"))
        })
        .transpose()
}

#[cfg(test)]
mod test {
    use super::*;

    fn config(toml: &str) -> Table {
        toml.parse().unwrap()
    }

    #[test]
    fn config_u16_reads_valid_values() {
        let config = config("port = 8085");
        assert_eq!(Some(8085), config_u16(&config, "port").unwrap());
        assert_eq!(None, config_u16(&config, "pool_size").unwrap());
    }

    #[test]
    fn config_u16_rejects_out_of_range_values() {
        for value in ["70000", "65537", "-1"] {
            let config = config(&format!("port = {value}"));
            let error = config_u16(&config, "port").unwrap_err();
            assert_eq!(
                format!("Expected `port` to be between 0 and 65535 but got {value}"),
                error.to_string()
            );
        }
    }

    #[test]
    fn config_u16_rejects_values_that_are_not_numbers() {
        let error = config_u16(&config("pool_size = \"ten\""), "pool_size").unwrap_err();
        assert_eq!("Expected `pool_size` to be a number", error.to_string());
    }
}
