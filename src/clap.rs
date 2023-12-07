use clap::{arg, command, value_parser, ArgMatches};
use std::path::PathBuf;

use lazy_static::lazy_static;

#[cfg(target_os = "windows")]
lazy_static! {
    pub static ref CONFIG_DIR: PathBuf =
        dirs::config_dir().unwrap_or(PathBuf::from("C:\\ProgramData"));
}

#[cfg(target_os = "macos")]
lazy_static! {
    pub static ref CONFIG_DIR: PathBuf = PathBuf::from("/Library/Application Support");
}
#[cfg(target_os = "linux")]
lazy_static! {
    pub static ref CONFIG_DIR: PathBuf = PathBuf::from("/etc");
}

lazy_static! {
    pub static ref CONFIG_FILE: PathBuf = CONFIG_DIR.join("toggleproxy.json");
}

pub fn get_args() -> ArgMatches {
    return command!()
        .about("A toggleable socks5 proxy")
        .version("1.0")
        .author("Luna")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            arg!(-c --config <FILE> "Sets a custom config file")
                .value_parser(value_parser!(String))
                .default_value(CONFIG_FILE.to_str().unwrap()),
        )
        .arg(arg!(-p --port <PORT> "Sets a custom port").value_parser(value_parser!(u16)))
        .arg(arg!(-t --target <TARGET> "Sets a custom target proxy"))
        .subcommand(command!("run").about("Starts the proxy server"))
        .subcommand(command!("toggle").about("Toggles the proxy server on or off"))
        .subcommand(command!("config").about("Writes the config file to disk"))
        .get_matches();
}
