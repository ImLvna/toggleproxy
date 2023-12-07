use crate::clap::get_args;

use std::path::Path;

use serde::{Deserialize, Serialize};

use anyhow::Result;

use log::{error, trace};

use notify::Watcher;

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub port: u16,
    pub target: String,
    pub status: bool,
    pub systemd: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 1080,
            target: "127.0.0.1:1081".to_string(),
            status: false,
            systemd: false,
        }
    }
}

pub fn get_real_config_path() -> String {
    let args = get_args();

    let config_path = args
        .get_one::<String>("config")
        .expect("No config file provided");

    return match Path::new(&config_path).is_dir() {
        true => match config_path.ends_with("/") {
            true => format!("{}{}", config_path, "toggleproxy.json"),
            false => format!("{}/{}", config_path, "toggleproxy.json"),
        },
        false => config_path.to_owned(),
    };
}

pub fn get_config() -> Config {
    let args = get_args();

    let config_path = get_real_config_path();

    let mut config: Config = match || -> Result<Config> {
        use std::fs::File;
        let file = match File::open(config_path) {
            Ok(file) => file,
            Err(err) => {
                error!("Failed to open config file");
                return Err(err.into());
            }
        };
        match serde_json::from_reader(file) {
            Ok(config) => Ok(config),
            Err(err) => {
                error!("Failed to parse config file");
                return Err(err.into());
            }
        }
    }() {
        Ok(config) => config,
        Err(err) => {
            trace!("{}", err);
            error!("Warning: Using default config");
            let config = Config::default();
            let _ = save_config(&config);
            config
        }
    };

    config.port = match args.get_one::<u16>("port") {
        Some(port) => *port,
        None => config.port,
    };

    config.target = match args.get_one::<String>("target") {
        Some(target) => target.to_owned(),
        None => config.target,
    };

    config.status = match args.get_one::<bool>("status") {
        Some(status) => *status,
        None => config.status,
    };

    config.systemd = match args.get_one::<bool>("systemd") {
        Some(systemd) => *systemd,
        None => config.systemd,
    };

    return config;
}

pub fn save_config(config: &Config) -> Result<()> {
    let config_path = get_real_config_path();

    let file = match std::fs::File::create(config_path) {
        Ok(file) => file,
        Err(err) => {
            error!("Failed to create config file");
            trace!("{}", err);
            return Err(anyhow::anyhow!("Failed to create config file"));
        }
    };
    match serde_json::to_writer_pretty(file, &config) {
        Ok(_) => {}
        Err(err) => {
            error!("Failed to write config file");
            trace!("{}", err);
            return Err(anyhow::anyhow!("Failed to write config file"));
        }
    };

    Ok(())
}

pub fn stringify_config(config: &Config) -> String {
    return serde_json::to_string_pretty(config).unwrap();
}
