// the rule settings type.
use engine::Config;
// types for the address and the time limit.
use std::{net::SocketAddr, time::Duration};

// shortest API key accepted, a short key is easy to guess.
const MIN_KEY_LEN: usize = 16;
// allowed difference between a transaction time and the server clock, in seconds.
const DEFAULT_CLOCK_SKEW: i64 = 300;
// where the server listens when nothing else is set, localhost only.
const DEFAULT_BIND: &str = "127.0.0.1:3000";
// longest time one request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
// most requests that may run at the same time.
const MAX_IN_FLIGHT: usize = 1024;

// everything the server needs to know before it starts.
pub struct Settings {
    // the key every protected request must carry in the x-api-key header.
    pub api_key: String,
    // where the server listens.
    pub bind: SocketAddr,
    // the Garnet address, empty means the in memory store is used.
    pub garnet_url: Option<String>,
    // the path of the rule settings file, empty means the defaults are used.
    pub engine_config: Option<String>,
    // allowed difference between a transaction time and the server clock, empty means no check.
    pub max_clock_skew: Option<i64>,
    // longest time one request may take.
    pub request_timeout: Duration,
    // most requests that may run at the same time.
    pub max_in_flight: usize,
}

impl Settings {
    // reads the settings from the environment variables of the process.
    pub fn from_env() -> Result<Self, String> {
        // look each name up in the real environment.
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    // reads the settings through any lookup function, so tests do not touch the real environment.
    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        // an empty variable counts as not set.
        let get = |name: &str| lookup(name).filter(|value| !value.is_empty());
        // the key is required, the server never runs open.
        let api_key = get("API_KEY").ok_or("API_KEY must be set")?;
        // a short key is refused.
        if api_key.len() < MIN_KEY_LEN {
            // say how long it must be.
            return Err(format!(
                "API_KEY must have at least {MIN_KEY_LEN} characters"
            ));
        }
        // the address, localhost unless told otherwise.
        let bind = get("BIND_ADDR")
            .unwrap_or_else(|| DEFAULT_BIND.to_string())
            .parse()
            .map_err(|_| "BIND_ADDR must look like 127.0.0.1:3000".to_string())?;
        // the clock check is on by default, off lets old data be replayed.
        let max_clock_skew = match get("CLOCK_SKEW_SECONDS").as_deref() {
            // not set means the default.
            None => Some(DEFAULT_CLOCK_SKEW),
            // the word off turns the check off.
            Some("off") => None,
            // anything else must be a positive number of seconds.
            Some(text) => match text.parse::<i64>() {
                // a good number.
                Ok(seconds) if seconds > 0 => Some(seconds),
                // a bad number.
                _ => return Err("CLOCK_SKEW_SECONDS must be a positive number or off".to_string()),
            },
        };
        // all settings together.
        Ok(Settings {
            api_key,
            bind,
            garnet_url: get("GARNET_URL"),
            engine_config: get("ENGINE_CONFIG"),
            max_clock_skew,
            request_timeout: REQUEST_TIMEOUT,
            max_in_flight: MAX_IN_FLIGHT,
        })
    }
}

// reads and checks the rule settings, without a path the defaults are used.
pub fn load_engine_config(path: Option<&str>) -> Result<Config, String> {
    // no file means the default values.
    let Some(path) = path else {
        // the defaults.
        return Ok(Config::default());
    };
    // read the file.
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
    // parse and check it.
    parse_engine_config(&text)
}

// turns the text of a settings file into checked rule settings.
fn parse_engine_config(text: &str) -> Result<Config, String> {
    // parse the file, unknown names and wrong types are errors.
    let config: Config = toml::from_str(text).map_err(|error| format!("engine config: {error}"))?;
    // check the values.
    config
        .validate()
        .map_err(|error| format!("engine config: {error}"))?;
    // good.
    Ok(config)
}

// tests for the settings.
#[cfg(test)]
mod tests {
    // bring the functions above into the tests.
    use super::*;

    // a lookup that answers from a fixed list instead of the real environment.
    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        // find the name in the list.
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    // the example file in the config folder must always match the defaults.
    #[test]
    fn example_file_matches_the_defaults() {
        // the example file is compiled into the test.
        let config = parse_engine_config(include_str!("../../../config/engine.toml")).unwrap();
        // Config has no equality, so the printed forms are compared.
        assert_eq!(format!("{config:?}"), format!("{:?}", Config::default()));
    }

    // a file may set only some values, but never an unknown name or a bad value.
    #[test]
    fn engine_config_files_are_checked() {
        // a partial file changes one value and keeps the rest.
        let config = parse_engine_config("review_threshold = 30").unwrap();
        // the changed value.
        assert_eq!(config.review_threshold, 30);
        // an untouched value.
        assert_eq!(config.block_threshold, 80);
        // a misspelled name is an error.
        assert!(parse_engine_config("reveiw_threshold = 30").is_err());
        // a value that fails the check is an error.
        assert!(parse_engine_config("block_threshold = 10").is_err());
        // a wrong type is an error.
        assert!(parse_engine_config("review_threshold = \"high\"").is_err());
        // a missing file is an error.
        assert!(load_engine_config(Some("does-not-exist.toml")).is_err());
        // no path gives the defaults.
        assert!(load_engine_config(None).is_ok());
    }

    // the key is required and long enough, the other settings have defaults and are checked.
    #[test]
    fn settings_are_read_and_checked() {
        // the key is missing.
        assert!(Settings::from_lookup(lookup(&[])).is_err());
        // the key is too short.
        assert!(Settings::from_lookup(lookup(&[("API_KEY", "short")])).is_err());
        // only a good key is set, so the defaults apply.
        let settings = Settings::from_lookup(lookup(&[("API_KEY", "0123456789abcdef")])).unwrap();
        // the default address is localhost.
        assert_eq!(settings.bind.to_string(), "127.0.0.1:3000");
        // the clock check is on.
        assert_eq!(settings.max_clock_skew, Some(300));
        // no Garnet address means no value.
        assert!(settings.garnet_url.is_none());
        // the clock check can be turned off.
        let off = Settings::from_lookup(lookup(&[
            ("API_KEY", "0123456789abcdef"),
            ("CLOCK_SKEW_SECONDS", "off"),
        ]))
        .unwrap();
        // no limit.
        assert_eq!(off.max_clock_skew, None);
        // a bad number is an error.
        assert!(
            Settings::from_lookup(lookup(&[
                ("API_KEY", "0123456789abcdef"),
                ("CLOCK_SKEW_SECONDS", "0")
            ]))
            .is_err()
        );
        // a bad address is an error.
        assert!(
            Settings::from_lookup(lookup(&[
                ("API_KEY", "0123456789abcdef"),
                ("BIND_ADDR", "nonsense")
            ]))
            .is_err()
        );
    }
}
