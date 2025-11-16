pub mod exchange;
pub mod manager;
pub mod message;
pub mod session;
pub mod threaded;
pub mod types;

use self::message::{DeserializeMessage, Message};
use crate::connection::exchange::DestinationInfo;
use crate::connection::exchange::MemoryRegionInfo;
use exchange::TestResults;
use sideway::ibverbs::device_context::DeviceContext;
use sideway::ibverbs::protection_domain::ProtectionDomain;
use sideway::ibverbs::queue_pair::ExtendedQueuePair;
use std::net::SocketAddr;
use std::time::Duration;

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
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("RDMA error: {0}")]
    RdmaError(String),
}

/// Generic result type for connection operations
pub type ConnectionResult<T> = Result<T, ConnectionError>;

/// Role of the endpoint in the connection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointRole {
    Server,
    Client,
}

/// Type of connection to establish
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionType {
    /// TCP connection (for out-of-band communication)
    Tcp,
    /// RDMA Connection Manager (future implementation)
    RdmaCm,
}

impl ConnectionType {
    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionType::Tcp => "tcp",
            ConnectionType::RdmaCm => "rdmacm",
        }
    }
}

impl std::fmt::Display for ConnectionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for ConnectionType {
    type Err = ConnectionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "tcp" => Ok(ConnectionType::Tcp),
            "rdmacm" | "rdma_cm" | "rdma-cm" => Ok(ConnectionType::RdmaCm),
            _ => Err(ConnectionError::InvalidConfiguration(format!(
                "Unknown connection type: '{}'",
                s
            ))),
        }
    }
}

/// Connection parameters
#[derive(Debug, Clone)]
pub struct ConnectionParams {
    pub timeout: Duration,
    pub retry_count: u32,
    pub private_data: Option<Vec<u8>>,
    pub role: EndpointRole,
    pub port: u16,
}

impl Default for ConnectionParams {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            retry_count: 3,
            private_data: None,
            role: EndpointRole::Client,
            port: 18515, // Default port
        }
    }
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

    /// Send raw bytes over the connection
    fn send_raw(&self, message_type: u32, payload: &[u8]) -> ConnectionResult<()>;

    /// Receive raw bytes from the connection
    fn receive_raw(&self) -> ConnectionResult<Vec<u8>>;

    /// Exchange QP information with peer
    fn exchange_qp_info(&self, local: DestinationInfo) -> Result<DestinationInfo, ConnectionError>;

    fn exchange_results(&self, local_results: TestResults) -> ConnectionResult<TestResults>;

    fn receive_results(&self) -> ConnectionResult<TestResults>;

    fn send_results(&self, results: &TestResults) -> ConnectionResult<()>;

    /// Exchange memory region information with peer
    fn exchange_memory_regions(
        &self,
        local_mr: MemoryRegionInfo,
    ) -> ConnectionResult<MemoryRegionInfo>;

    /// Get the local address
    fn local_addr(&self) -> Result<SocketAddr, ConnectionError>;

    /// Get the peer address
    fn peer_addr(&self) -> Result<SocketAddr, ConnectionError>;

    /// Set connection parameters
    fn set_params(&mut self, params: ConnectionParams);

    /// Get connection parameters
    fn params(&self) -> &ConnectionParams;

    /// Close the connection
    fn close(&mut self) -> Result<(), ConnectionError>;

    /// Setup QP with the remote information
    fn setup_qp(
        &self,
        ctx: &DeviceContext,
        pd: &ProtectionDomain,
        qp: &mut ExtendedQueuePair,
        local_data: DestinationInfo,
    ) -> ConnectionResult<DestinationInfo>;
}

pub struct ConnectionFactory;

impl ConnectionFactory {
    pub fn create(
        conn_type: ConnectionType,
        params: ConnectionParams,
    ) -> ConnectionResult<Box<dyn ConnectionManager>> {
        match conn_type {
            ConnectionType::Tcp => Ok(Box::new(manager::tcp::TcpConnectionManager::new(params))),
            ConnectionType::RdmaCm => Err(ConnectionError::InvalidConfiguration(
                "RDMA CM not implemented yet".to_string(),
            )),
        }
    }
}

pub trait ConnectionManagerExt: ConnectionManager {
    fn send_message<T: Message>(&self, message_type: u32, payload: &T) -> ConnectionResult<()> {
        let serialized = payload.serialize()?;
        self.send_raw(message_type, &serialized)
    }

    fn receive_message<T: DeserializeMessage>(&self) -> ConnectionResult<T> {
        let data = self.receive_raw()?;
        T::deserialize(&data)
    }

    fn exchange_message<T: Message + DeserializeMessage>(
        &self,
        message_type: u32,
        payload: &T,
    ) -> ConnectionResult<T> {
        self.send_message(message_type, payload)?;
        self.receive_message()
    }
}

impl<T: ?Sized + ConnectionManager> ConnectionManagerExt for T {}
