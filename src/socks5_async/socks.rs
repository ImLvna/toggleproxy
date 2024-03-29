#[allow(dead_code)]
use std::{
    error::Error,
    fmt,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, ToSocketAddrs},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};

// Const bytes
pub const VERSION5: u8 = 0x05;
pub const RESERVED: u8 = 0x00;

// Request command
pub enum Command {
    Connect = 0x01,
    Bind = 0x02,
    UdpAssosiate = 0x3,
}
impl Command {
    pub fn from(byte: usize) -> Option<Command> {
        match byte {
            1 => Some(Command::Connect),
            2 => Some(Command::Bind),
            3 => Some(Command::UdpAssosiate),
            _ => None,
        }
    }
}

// Request address type
#[derive(PartialEq)]
pub enum AddrType {
    V4 = 0x01,
    Domain = 0x03,
    V6 = 0x04,
}
impl AddrType {
    pub fn from(byte: usize) -> Option<AddrType> {
        match byte {
            1 => Some(AddrType::V4),
            3 => Some(AddrType::Domain),
            4 => Some(AddrType::V6),
            _ => None,
        }
    }

    pub async fn get_socket_addrs<S: AsyncRead + AsyncWrite + Unpin>(
        socket: &mut S,
    ) -> Result<Vec<SocketAddr>, Box<dyn Error>> {
        // Read address type
        let mut addr_type = [0u8; 1];
        socket.read(&mut addr_type).await?;
        let addr_type = AddrType::from(addr_type[0] as usize);
        if let None = addr_type {
            Err(Response::AddrTypeNotSupported)?;
        }
        let addr_type = addr_type.unwrap();

        // Read address
        let addr;
        if let AddrType::Domain = addr_type {
            let mut dlen = [0u8; 1];
            socket.read_exact(&mut dlen).await?;
            let mut domain = vec![0u8; dlen[0] as usize];
            socket.read_exact(&mut domain).await?;
            addr = domain;
        } else if let AddrType::V4 = addr_type {
            let mut v4 = [0u8; 4];
            socket.read_exact(&mut v4).await?;
            addr = Vec::from(v4);
        } else {
            let mut v6 = [0u8; 16];
            socket.read_exact(&mut v6).await?;
            addr = Vec::from(v6);
        }

        // Read port
        let mut port = [0u8; 2];
        socket.read_exact(&mut port).await?;
        let port = (u16::from(port[0]) << 8) | u16::from(port[1]);

        // Return socket address vector
        match addr_type {
            AddrType::V6 => {
                let new_addr = (0..8)
                    .map(|x| (u16::from(addr[x * 2]) << 8) | u16::from(addr[(x * 2) + 1]))
                    .collect::<Vec<u16>>();
                Ok(vec![SocketAddr::from(SocketAddrV6::new(
                    Ipv6Addr::new(
                        new_addr[0],
                        new_addr[1],
                        new_addr[2],
                        new_addr[3],
                        new_addr[4],
                        new_addr[5],
                        new_addr[6],
                        new_addr[7],
                    ),
                    port,
                    0,
                    0,
                ))])
            }
            AddrType::V4 => Ok(vec![SocketAddr::from(SocketAddrV4::new(
                Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3]),
                port,
            ))]),
            AddrType::Domain => {
                let mut domain = String::from_utf8_lossy(&addr[..]).to_string();
                domain.push_str(&":");
                domain.push_str(&port.to_string());
                Ok(domain.to_socket_addrs()?.collect())
            }
        }
    }
}

// Server response codes
#[allow(dead_code)]
#[derive(Debug)]
pub enum Response {
    Success = 0x00,
    Failure = 0x01,
    RuleFailure = 0x02,
    NetworkUnreachable = 0x03,
    HostUnreachable = 0x04,
    ConnectionRefused = 0x05,
    TtlExpired = 0x06,
    CommandNotSupported = 0x07,
    AddrTypeNotSupported = 0x08,
}
impl Error for Response {}
impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Error: {}", self)
    }
}

/// Available authentication methods and their hex value
#[derive(PartialEq)]
pub enum AuthMethod {
    NoAuth = 0x00,
    UserPass = 0x02,
    NoMethods = 0xFF,
}
impl AuthMethod {
    fn from(byte: u8) -> AuthMethod {
        if byte == (AuthMethod::NoAuth as u8) {
            AuthMethod::NoAuth
        } else if byte == (AuthMethod::UserPass as u8) {
            AuthMethod::UserPass
        } else {
            AuthMethod::NoMethods
        }
    }
    pub async fn get_available_methods<S: AsyncRead + AsyncWrite + Unpin>(
        methods_count: u8,
        socket: &mut S,
    ) -> Result<Vec<AuthMethod>, Box<dyn Error>> {
        let mut methods: Vec<AuthMethod> = Vec::with_capacity(methods_count as usize);
        for _ in 0..methods_count {
            let mut method = [0u8; 1];
            socket.read_exact(&mut method).await?;
            methods.push(AuthMethod::from(method[0]));
        }
        Ok(methods)
    }
}
