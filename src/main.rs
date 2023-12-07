use config::get_config;

use crate::{
    clap::get_args,
    config::{save_config, stringify_config},
    server::server,
};

pub mod clap;
pub mod config;
pub mod server;
pub mod socks5_async;
pub mod systemd;

#[tokio::main]
async fn main() {
    simple_logger::init().unwrap();

    let mut config = get_config();
    let args = get_args();

    match args.subcommand() {
        Some(("run", _)) => {
            println!("Running proxy server on port {}", config.port);
            match server(config).await {
                Ok(_) => {}
                Err(err) => {
                    println!("Failed to run proxy server: {}", err);
                }
            }
        }
        Some(("toggle", _)) => {
            config.status = args
                .get_one::<bool>("status")
                .unwrap_or(&!config.status)
                .to_owned();
            println!(
                "Proxy server is now {}",
                match config.status {
                    true => "on",
                    false => "off",
                }
            );
            match save_config(&config) {
                Ok(_) => {
                    if config.systemd {
                        match systemd::systemd_restart() {
                            Ok(_) => {
                                println!("Systemd service restarted");
                            }
                            Err(err) => {
                                println!("Failed to restart systemd service: {}", err);
                            }
                        }
                    }
                }
                Err(err) => {
                    println!("Failed to save config: {}", err);
                }
            }
        }
        Some(("config", _)) => match save_config(&config) {
            Ok(_) => {
                println!("Config saved");
                println!("The config is now:\n{}", stringify_config(&config));

                if config.systemd {
                    match systemd::systemd_restart() {
                        Ok(_) => {
                            println!("Systemd service restarted");
                        }
                        Err(err) => {
                            println!("Failed to restart systemd service: {}", err);
                        }
                    }
                }
            }
            Err(err) => {
                println!("Failed to save config: {}", err);
            }
        },
        _ => {}
    }
}
