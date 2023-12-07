use std::{net::SocketAddr, sync::Arc};

use log::error;
use tokio::{io::AsyncWriteExt, net::TcpListener, net::TcpStream};

use crate::{config::Config, socks5_async::lib::TargetAddr};

use tokio::io::copy_bidirectional;

use anyhow::Result;

use socks5_server::{
    auth::NoAuth, connection::state::NeedCommand, Command, IncomingConnection, Server,
};

use socks5_proto::{Address, Reply};

use crate::socks5_async::lib::SocksStream;

pub async fn server(config: Config) -> Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", config.port)).await?;

    let auth = Arc::new(NoAuth) as Arc<_>;

    let server = Server::new(listener, auth);

    while let Ok((conn, _)) = server.accept().await {
        let config = config.clone();
        tokio::spawn(async move {
            match conn.authenticate().await {
                Ok((conn, _)) => match handle(conn, config).await {
                    Ok(()) => {}
                    Err(err) => error!("Failed to execute command: {:?}", err),
                },
                Err(err) => error!("Failed to authenticate connection: {:?}", err),
            }
        });
    }

    Ok(())
}

async fn handle(conn: IncomingConnection<(), NeedCommand>, config: Config) -> Result<()> {
    println!("Connected");
    match conn.wait().await {
        // Handle connect command
        Ok(Command::Connect(connect, addr)) => {
            let target = match config.status {
                false => match addr.clone() {
                    Address::SocketAddress(addr) => TcpStream::connect(addr).await,
                    Address::DomainAddress(domain, port) => {
                        TcpStream::connect((String::from_utf8(domain).unwrap(), port)).await
                    }
                },
                true => {
                    SocksStream::connect(
                        config.target.parse::<SocketAddr>().unwrap(),
                        match addr.clone() {
                            Address::SocketAddress(addr) => match addr {
                                SocketAddr::V4(addr) => TargetAddr::V4(addr),
                                SocketAddr::V6(addr) => TargetAddr::V6(addr),
                            },
                            Address::DomainAddress(domain, port) => {
                                TargetAddr::Domain((String::from_utf8(domain).unwrap(), port))
                            }
                        },
                        None,
                    )
                    .await
                }
            };

            match target {
                Ok(mut target) => {
                    let reply = connect.reply(Reply::Succeeded, addr).await;

                    let mut conn = match reply {
                        Ok(conn) => conn,
                        Err((err, mut conn)) => {
                            let _ = conn.shutdown().await;
                            return Err(err.into());
                        }
                    };

                    let _ = copy_bidirectional(&mut target, &mut conn).await;
                    let _ = conn.shutdown().await;
                    let _ = target.shutdown().await;
                }
                Err(err) => {
                    error!("Failed to connect to target: {:?}", err);
                    let mut conn = match connect
                        .reply(Reply::HostUnreachable, Address::unspecified())
                        .await
                    {
                        Ok(conn) => conn,
                        Err((err, mut conn)) => {
                            let _ = conn.shutdown().await;
                            return Err(err.into());
                        }
                    };
                    let _ = conn.close().await;
                }
            }
        }

        // Kill unknown commands
        Ok(Command::Associate(cmd, _)) => {
            let mut conn = match cmd
                .reply(Reply::CommandNotSupported, Address::unspecified())
                .await
            {
                Ok(conn) => conn,
                Err((err, mut conn)) => {
                    let _ = conn.shutdown().await;
                    return Err(err.into());
                }
            };
            let _ = conn.close().await;
        }
        Ok(Command::Bind(cmd, _)) => {
            let mut conn = match cmd
                .reply(Reply::CommandNotSupported, Address::unspecified())
                .await
            {
                Ok(conn) => conn,
                Err((err, mut conn)) => {
                    let _ = conn.shutdown().await;
                    return Err(err.into());
                }
            };
            let _ = conn.close().await;
        }

        // Kill errors
        Err((err, mut conn)) => {
            print!("hit");
            let _ = conn.shutdown().await;
            return Err(err.into());
        }
    }
    Ok(())
}
