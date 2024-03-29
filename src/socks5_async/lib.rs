pub use crate::socks5_async::socks::AuthMethod;
use crate::socks5_async::socks::{AddrType, Command, Response, RESERVED, VERSION5};
use futures::future::try_join;
use std::{
    boxed::Box,
    error::Error,
    io,
    net::{SocketAddr, SocketAddrV4, SocketAddrV6},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
};

use log::{error, info, warn};

// Transmited over mpsc channel to check user authentication
type AuthCheckMsg = (String, String, oneshot::Sender<bool>);

use anyhow::Result;

/// A SOCKS5 Server
pub struct SocksServer {
    listener: TcpListener,
    allow_no_auth: bool,
    auth_tx: mpsc::Sender<AuthCheckMsg>,
}
impl SocksServer {
    /// Creates and returns a new `SocksServer`
    pub async fn new(
        socket_addr: SocketAddr,
        allow_no_auth: bool,
        auth: Box<dyn Fn(String, String) -> bool + Send>,
    ) -> SocksServer {
        let (tx, mut rx) = mpsc::channel::<AuthCheckMsg>(100);
        tokio::spawn(async move {
            while let Some((username, password, sender)) = rx.recv().await {
                if let Err(_) = sender.send(auth(username, password)) {
                    error!("Failed to send back authentication result.");
                }
            }
        });
        println!("SOCKS5 server listening on {}", socket_addr);
        SocksServer {
            listener: TcpListener::bind(socket_addr).await.unwrap(),
            allow_no_auth,
            auth_tx: tx,
        }
    }

    /// Starts the server. It **should** be called after initializing server
    ///
    /// # Example
    /// ```
    /// use socks5_async::SocksServer;
    /// use std::{
    ///     boxed::Box,
    ///     error::Error,
    ///     net::SocketAddr,
    /// };
    ///
    /// let users = vec![
    ///     (String::from("user1"), String::from("123456"))
    /// ];
    ///
    /// // Server address
    /// let address: SocketAddr = "127.0.0.1:1080".parse().unwrap();
    /// let mut socks5 = SocksServer::new(address, true,
    ///     Box::new(move |username, password| {
    ///         // Authenticate user
    ///         return users.contains(&(username, password));
    ///     }),
    /// ).await;
    /// socks5.serve().await;
    ///
    /// ```

    pub async fn serve(&mut self) {
        loop {
            let no_auth = self.allow_no_auth.clone();
            if let Ok((socket, address)) = self.listener.accept().await {
                let tx2 = self.auth_tx.clone();
                tokio::spawn(async move {
                    info!("Client connected: {}", address);
                    let mut client = SocksServerConnection::new(socket, no_auth, tx2);
                    match client.serve().await {
                        Ok(_) => info!("Request was served successfully."),
                        Err(err) => error!("{}", err.to_string()),
                    }
                });
            }
        }
    }
}

// Represents a SOCKS5 Client (connected to SocksServer)
struct SocksServerConnection {
    socket: TcpStream,
    no_auth: bool,
    auth_ch: mpsc::Sender<AuthCheckMsg>,
}
impl SocksServerConnection {
    fn new(
        socket: TcpStream,
        no_auth: bool,
        auth_ch: mpsc::Sender<(String, String, oneshot::Sender<bool>)>,
    ) -> SocksServerConnection {
        SocksServerConnection {
            socket,
            no_auth,
            auth_ch,
        }
    }

    fn shutdown(&mut self, msg: &str) -> Result<(), Box<dyn Error>> {
        warn!("{}", msg);
        Ok(())
    }

    async fn serve(&mut self) -> Result<(), Box<dyn Error>> {
        let mut header = [0u8; 2];
        self.socket.read_exact(&mut header).await?;

        // Accept only version 5
        if header[0] != VERSION5 {
            self.shutdown("Unsupported version")?;
            Err(Response::Failure)?;
        }

        // Get available methods
        let methods = AuthMethod::get_available_methods(header[1], &mut self.socket).await?;

        // Authenticate the user
        self.auth(methods).await?;

        // Handle the request
        self.handle_req().await?;

        Ok(())
    }

    async fn auth(&mut self, methods: Vec<AuthMethod>) -> Result<(), Box<dyn Error>> {
        if methods.contains(&AuthMethod::UserPass) {
            // Authenticate with username/password
            self.socket
                .write_all(&[VERSION5, AuthMethod::UserPass as u8])
                .await?;

            // Read username
            let mut ulen = [0u8; 2];
            self.socket.read_exact(&mut ulen).await?;
            let ulen = ulen[1];
            let mut username: Vec<u8> = Vec::with_capacity(ulen as usize);
            for _ in 0..ulen {
                username.push(0)
            }
            self.socket.read_exact(&mut username).await?;
            let username = String::from_utf8(username).unwrap();

            // Read Password
            let mut plen = [0u8; 1];
            self.socket.read_exact(&mut plen).await?;
            let plen = plen[0];
            let mut password: Vec<u8> = Vec::with_capacity(plen as usize);
            for _ in 0..plen {
                password.push(0)
            }
            self.socket.read_exact(&mut password).await?;
            let password = String::from_utf8(password).unwrap();

            // Authenticate user
            let (tx, rx) = oneshot::channel::<bool>();
            self.auth_ch.send((username.clone(), password, tx)).await?;
            if rx.await? {
                info!("User authenticated: {}", username);
                self.socket.write_all(&[1, Response::Success as u8]).await?;
            } else {
                self.socket
                    .write_all(&[VERSION5, Response::Failure as u8])
                    .await?;
                self.shutdown("Authentication failed.")?;
            }
        } else if self.no_auth && methods.contains(&AuthMethod::NoAuth) {
            warn!("Client connected with no authentication");
            self.socket
                .write_all(&[VERSION5, AuthMethod::NoAuth as u8])
                .await?
        } else {
            self.socket
                .write_all(&[VERSION5, Response::Failure as u8])
                .await?;
            self.shutdown("No acceptable method found.")?;
        }
        Ok(())
    }

    async fn handle_req(&mut self) -> Result<(), Box<dyn Error>> {
        // Read request header
        let mut data = [0u8; 3];
        self.socket.read(&mut data).await?;

        // Read socket address
        let addresses = AddrType::get_socket_addrs(&mut self.socket).await?;

        // Proccess the command
        match Command::from(data[1] as usize) {
            // Note: Currently only connect is accepted
            Some(Command::Connect) => self.cmd_connect(addresses).await?,
            _ => {
                self.shutdown("Command not supported.")?;
                Err(Response::CommandNotSupported)?;
            }
        };

        Ok(())
    }

    async fn cmd_connect(&mut self, addrs: Vec<SocketAddr>) -> Result<(), Box<dyn Error>> {
        let mut dest = TcpStream::connect(&addrs[..]).await?;

        self.socket
            .write_all(&[
                VERSION5,
                Response::Success as u8,
                RESERVED,
                1,
                127,
                0,
                0,
                1,
                0,
                0,
            ])
            .await
            .unwrap();

        let (mut ro, mut wo) = dest.split();
        let (mut ri, mut wi) = self.socket.split();

        let client_to_server = async {
            tokio::io::copy(&mut ri, &mut wo).await?;
            wo.shutdown().await
        };

        let server_to_client = async {
            tokio::io::copy(&mut ro, &mut wi).await?;
            wi.shutdown().await
        };

        try_join(client_to_server, server_to_client).await?;

        Ok(())
    }
}

/// A SOCKS5 Stream
pub struct SocksStream {
    stream: TcpStream,
}
impl SocksStream {
    /// Connects to `proxy_addr` and returns a `TcpStream` which
    /// is authenticated via provided methods and ready to transfer data.
    ///
    /// # Example
    /// ```
    /// use socks5_async::SocksStream;
    ///
    /// // SOCKS5 proxy server address
    /// let proxy: SocketAddr = "127.0.0.1:1080".parse().unwrap();
    ///
    /// // Target address
    /// let target: SocketAddrV4 = "127.0.0.1:3033".parse().unwrap();
    ///
    /// // Connect to server
    /// let stream = SocksStream::connect(
    ///     proxy,
    ///     target,
    ///     // Pass None if you want to use NoAuth method
    ///     Some(("user1".to_string(), "123456".to_string())),
    /// ).await?;
    ///
    /// // Use tcp stream ...
    /// ```
    /// # Note
    /// This methods uses `connect_with_stream()` under the hood.
    pub async fn connect(
        proxy_addr: SocketAddr,
        target_addr: impl ToTargetAddr,
        user_pass: Option<(String, String)>,
    ) -> Result<TcpStream, std::io::Error> {
        let mut socks_stream = SocksStream {
            stream: TcpStream::connect(proxy_addr).await?,
        };
        match connect_with_stream(&mut socks_stream.stream, target_addr, user_pass).await {
            Ok(_) => Ok(socks_stream.stream),
            Err(err) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                err.to_string(),
            )),
        }
    }
}

/// Perform SOCKS5 handshake through a TCP stream
pub async fn socks_handshake(
    stream: &mut TcpStream,
    user_pass: Option<(String, String)>,
) -> Result<(), Box<dyn Error>> {
    let with_userpass = user_pass.is_some();
    let methods_len = if with_userpass { 2 } else { 1 };

    // Start SOCKS5 communication
    let mut data = vec![0; methods_len + 2];
    data[0] = VERSION5; // Set SOCKS version
    data[1] = methods_len as u8; // Set authentiaction methods count
    if with_userpass {
        data[2] = AuthMethod::UserPass as u8;
    }
    data[1 + methods_len] = AuthMethod::NoAuth as u8;
    stream.write_all(&mut data).await?;

    // Read method selection response
    let mut response = [0u8; 2];
    stream.read_exact(&mut response).await?;

    // Check SOCKS version
    if response[0] != VERSION5 {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid SOCKS version",
        ))?;
    }

    if response[1] == AuthMethod::UserPass as u8 {
        if let Some((username, password)) = user_pass {
            // Send username & password
            let mut data = vec![0; username.len() + password.len() + 3];
            data[0] = VERSION5;
            data[1] = username.len() as u8;
            data[2..2 + username.len()].copy_from_slice(username.as_bytes());
            data[2 + username.len()] = password.len() as u8;
            data[3 + username.len()..].copy_from_slice(password.as_bytes());
            stream.write_all(&data).await?;

            // Read & check server response
            let mut response = [0; 2];
            stream.read_exact(&mut response).await?;
            if response[1] != Response::Success as u8 {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Wrong username/password",
                ))?;
            }
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "Username & password requried",
            ))?;
        }
    } else if response[1] != AuthMethod::NoAuth as u8 {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Invalid authentication method",
        ))?;
    }

    Ok(())
}

/// Send `CONNECT` command to a SOCKS server
pub async fn cmd_connect(
    stream: &mut TcpStream,
    target_addr: impl ToTargetAddr,
) -> Result<(), Box<dyn Error>> {
    let target_addr = target_addr.target_addr();

    // Send connect command
    let mut data = vec![0; 6 + target_addr.len()];
    data[0] = VERSION5;
    data[1] = Command::Connect as u8;
    data[2] = RESERVED;
    data[3] = target_addr.addr_type() as u8;
    target_addr.write_to(&mut data[4..]);
    stream.write_all(&data).await?;

    // Read server response
    let mut response = [0u8; 3];
    stream.read(&mut response).await?;

    // Read socket address
    AddrType::get_socket_addrs(stream).await?;

    Ok(())
}

/// Perform SOCKS5 handshake and send `CONNECT` command through a TCP stream
pub async fn connect_with_stream(
    stream: &mut TcpStream,
    target_addr: impl ToTargetAddr,
    user_pass: Option<(String, String)>,
) -> Result<(), Box<dyn Error>> {
    socks_handshake(stream, user_pass).await?;
    cmd_connect(stream, target_addr).await?;

    Ok(())
}

/// Socket Address of the target, required by `SocksStream`
#[derive(Debug, Clone)]
pub enum TargetAddr {
    V4(SocketAddrV4),
    V6(SocketAddrV6),
    Domain((String, u16)),
}
impl TargetAddr {
    fn len(&self) -> usize {
        match self {
            TargetAddr::V4(_) => 4,
            TargetAddr::V6(_) => 16,
            TargetAddr::Domain((domain, _)) => domain.len() + 1,
        }
    }
    fn addr_type(&self) -> AddrType {
        match self {
            TargetAddr::V4(_) => AddrType::V4,
            TargetAddr::V6(_) => AddrType::V4,
            TargetAddr::Domain(_) => AddrType::Domain,
        }
    }
    fn write_to(&self, buf: &mut [u8]) {
        match self {
            TargetAddr::V4(addr) => {
                let mut ip = addr.ip().octets().to_vec();
                ip.extend(&addr.port().to_be_bytes());
                buf[..].copy_from_slice(&ip[..]);
            }
            TargetAddr::V6(addr) => {
                let mut ip = addr.ip().octets().to_vec();
                ip.extend(&addr.port().to_be_bytes());
                buf[..].copy_from_slice(&ip[..]);
            }
            TargetAddr::Domain((domain, port)) => {
                let mut ip = domain.as_bytes().to_vec();
                ip.extend(&port.to_be_bytes());
                buf[0] = domain.len() as u8;
                buf[1..].copy_from_slice(&ip[..]);
            }
        }
    }
}

/// A trait implemented by types that can be converted to `TargetAddr`
pub trait ToTargetAddr {
    fn target_addr(self) -> TargetAddr;
}

impl ToTargetAddr for TargetAddr {
    fn target_addr(self) -> TargetAddr {
        self
    }
}

impl ToTargetAddr for SocketAddrV4 {
    fn target_addr(self) -> TargetAddr {
        TargetAddr::V4(self)
    }
}

impl ToTargetAddr for SocketAddrV6 {
    fn target_addr(self) -> TargetAddr {
        TargetAddr::V6(self)
    }
}

impl ToTargetAddr for SocketAddr {
    fn target_addr(self) -> TargetAddr {
        match self {
            SocketAddr::V4(addr) => TargetAddr::V4(addr),
            SocketAddr::V6(addr) => TargetAddr::V6(addr),
        }
    }
}
