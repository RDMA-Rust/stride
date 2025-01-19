pub mod exchange;

// src/connection/mod.rs
pub mod manager;
pub mod types;

use std::net::SocketAddr;

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("Failed to resolve address: {0}")]
    AddressResolution(String),
    #[error("Connection refused: {0}")]
    ConnectionRefused(String),
    #[error("Timeout: {0}")]
    Timeout(String),
    #[error("Exchange failed: {0}")]
    ExchangeFailed(String),
}

#[derive(Debug, Clone)]
pub struct ConnectionParams {
    pub timeout_ms: u32,
    pub retry_count: u32,
    pub private_data: Option<Vec<u8>>,
    // ... other parameters
}

#[derive(Debug)]
pub struct ExchangeData {
    pub qp_num: u32,
    pub lid: u16,
    pub gid: Option<[u8; 16]>,
    pub psn: u32,
    // ... other QP info to exchange
}
pub trait ConnectionManager: Send + Sync {
    /// Initialize the connection manager
    fn init(&mut self) -> Result<(), ConnectionError>;

    /// Listen for incoming connections
    fn listen(&mut self, addr: SocketAddr) -> Result<(), ConnectionError>;

    /// Connect to a remote peer
    fn connect(&mut self, addr: SocketAddr) -> Result<(), ConnectionError>;

    /// Accept an incoming connection
    fn accept(&mut self) -> Result<(), ConnectionError>;

    /// Exchange QP information with peer
    fn exchange_qp_info(&self, local: ExchangeData) -> Result<ExchangeData, ConnectionError>;

    /// Get the local address
    fn local_addr(&self) -> Result<SocketAddr, ConnectionError>;

    /// Get the peer address
    fn peer_addr(&self) -> Result<SocketAddr, ConnectionError>;

    /// Set connection parameters
    fn set_params(&mut self, params: ConnectionParams);

    /// Close the connection
    fn close(&mut self) -> Result<(), ConnectionError>;
}
