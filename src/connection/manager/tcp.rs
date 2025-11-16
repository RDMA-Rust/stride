use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use std::io::{Read, Write};

use crate::connection::exchange::DestinationInfo;
use crate::connection::exchange::MemoryRegionInfo;
use crate::connection::exchange::TestResults;
use crate::connection::manager::{ConnectionId, ConnectionInfo};
use crate::connection::{ConnectionError, ConnectionManager, ConnectionParams, ConnectionResult};

use sideway::ibverbs::address::AddressHandleAttribute;
use sideway::ibverbs::device_context::DeviceContext;
use sideway::ibverbs::protection_domain::ProtectionDomain;
use sideway::ibverbs::queue_pair::QueuePairAttribute;
use sideway::ibverbs::queue_pair::QueuePairState;
use sideway::ibverbs::queue_pair::{ExtendedQueuePair, QueuePair};
use sideway::ibverbs::AccessFlags;

use serde::{Deserialize, Serialize};

const _CONNECTION_PROTOCOL_VERSION: u32 = 1;

/// Header for connection protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MessageHeader {
    protocol_version: u32,
    message_type: u32,
    payload_length: u32,
}

#[derive(Debug)]
pub struct TcpConnectionManager {
    params: ConnectionParams,
    listener: Option<Arc<Mutex<TcpListener>>>,
    stream: Option<Arc<Mutex<TcpStream>>>,
    local_addr: Option<SocketAddr>,
    peer_addr: Option<SocketAddr>,
}

impl TcpConnectionManager {
    pub fn new(params: ConnectionParams) -> Self {
        Self {
            params,
            listener: None,
            stream: None,
            local_addr: None,
            peer_addr: None,
        }
    }
}

impl ConnectionManager for TcpConnectionManager {
    fn init(&mut self) -> ConnectionResult<()> {
        // Nothing special to initialize for TCP
        Ok(())
    }

    fn listen(&mut self, addr: SocketAddr) -> ConnectionResult<()> {
        let listener = TcpListener::bind(addr).map_err(ConnectionError::IoError)?;

        listener
            .set_nonblocking(false)
            .map_err(ConnectionError::IoError)?;

        self.local_addr = Some(listener.local_addr().map_err(ConnectionError::IoError)?);

        self.listener = Some(Arc::new(Mutex::new(listener)));

        Ok(())
    }

    fn connect(&mut self, addr: SocketAddr) -> ConnectionResult<()> {
        let mut retries = 0;
        let mut last_error = None;

        while retries < self.params.retry_count {
            match TcpStream::connect_timeout(&addr, self.params.timeout) {
                Ok(stream) => {
                    stream.set_nodelay(true).map_err(ConnectionError::IoError)?;

                    self.local_addr = Some(stream.local_addr().map_err(ConnectionError::IoError)?);
                    self.peer_addr = Some(addr);
                    self.stream = Some(Arc::new(Mutex::new(stream)));

                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(e);
                    retries += 1;
                    std::thread::sleep(Duration::from_millis((100 * retries).into()));
                }
            }
        }

        Err(ConnectionError::ConnectionRefused(format!(
            "Failed to connect after {} retries: {:?}",
            self.params.retry_count, last_error
        )))
    }

    fn accept(&mut self) -> ConnectionResult<()> {
        let listener = self.listener.as_ref().ok_or_else(|| {
            ConnectionError::InvalidConfiguration("No active listener".to_string())
        })?;

        let (stream, peer_addr) = listener
            .lock()
            .unwrap()
            .accept()
            .map_err(ConnectionError::IoError)?;

        stream.set_nodelay(true).map_err(ConnectionError::IoError)?;

        self.peer_addr = Some(peer_addr);
        self.stream = Some(Arc::new(Mutex::new(stream)));

        Ok(())
    }

    fn exchange_qp_info(&self, local_data: DestinationInfo) -> ConnectionResult<DestinationInfo> {
        // Serialize with bincode directly
        let serialized = bincode::serialize(&local_data)
            .map_err(|e| ConnectionError::SerializationError(e.to_string()))?;

        // Send raw data
        self.send_raw(1, &serialized)?;

        // Receive raw data
        let data = self.receive_raw()?;

        // Deserialize
        let remote_data = bincode::deserialize(&data)
            .map_err(|e| ConnectionError::SerializationError(e.to_string()))?;

        Ok(remote_data)
    }

    fn exchange_results(&self, local_data: TestResults) -> ConnectionResult<TestResults> {
        let serialized = bincode::serialize(&local_data)
            .map_err(|e| ConnectionError::SerializationError(e.to_string()))?;

        // Send raw data
        self.send_raw(1, &serialized)?;

        // Receive raw data
        let data = self.receive_raw()?;

        // Deserialize
        let remote_data = bincode::deserialize(&data)
            .map_err(|e| ConnectionError::SerializationError(e.to_string()))?;

        Ok(remote_data)
    }

    // Method for the server to just receive results (no exchange needed)
    fn receive_results(&self) -> ConnectionResult<TestResults> {
        // Receive data
        let data = self.receive_raw()?;

        // Deserialize
        let results = bincode::deserialize(&data)
            .map_err(|e| ConnectionError::SerializationError(e.to_string()))?;

        Ok(results)
    }

    // Method for the client to just send results (no exchange needed)
    fn send_results(&self, results: &TestResults) -> ConnectionResult<()> {
        // Serialize
        let serialized = bincode::serialize(results)
            .map_err(|e| ConnectionError::SerializationError(e.to_string()))?;

        // Send with message type 4
        self.send_raw(4, &serialized)
    }

    fn exchange_memory_regions(
        &self,
        local_mr: MemoryRegionInfo,
    ) -> ConnectionResult<MemoryRegionInfo> {
        // Serialize with bincode
        let serialized = bincode::serialize(&local_mr)
            .map_err(|e| ConnectionError::SerializationError(e.to_string()))?;

        // Send local MR info with message type 3
        self.send_raw(3, &serialized)?;

        // Receive remote MR info
        let data = self.receive_raw()?;

        // Deserialize
        let remote_mr = bincode::deserialize(&data)
            .map_err(|e| ConnectionError::SerializationError(e.to_string()))?;

        Ok(remote_mr)
    }

    fn local_addr(&self) -> ConnectionResult<SocketAddr> {
        self.local_addr.ok_or_else(|| {
            ConnectionError::InvalidConfiguration("No local address available".to_string())
        })
    }

    fn peer_addr(&self) -> ConnectionResult<SocketAddr> {
        self.peer_addr.ok_or_else(|| {
            ConnectionError::InvalidConfiguration("No peer address available".to_string())
        })
    }

    fn set_params(&mut self, params: ConnectionParams) {
        self.params = params;
    }

    fn params(&self) -> &ConnectionParams {
        &self.params
    }

    fn close(&mut self) -> ConnectionResult<()> {
        self.stream = None;
        self.listener = None;
        self.local_addr = None;
        self.peer_addr = None;

        Ok(())
    }

    fn setup_qp(
        &self,
        _ctx: &DeviceContext,
        _pd: &ProtectionDomain,
        qp: &mut ExtendedQueuePair,
        local_data: DestinationInfo,
    ) -> ConnectionResult<DestinationInfo> {
        // Exchange QP information
        let remote_data = self.exchange_qp_info(local_data).unwrap();

        // Transition QP to Init state
        let mut attr = QueuePairAttribute::new();
        attr.setup_state(QueuePairState::Init)
            .setup_pkey_index(0)
            .setup_port(1)
            .setup_access_flags(
                AccessFlags::LocalWrite | AccessFlags::RemoteWrite | AccessFlags::RemoteRead,
            );

        qp.modify(&attr).map_err(|e| {
            ConnectionError::RdmaError(format!("Failed to move QP to INIT: {:?}", e))
        })?;

        // Parse GID from the remote data
        let gid = remote_data.gid;

        // Transition QP to RTR state
        let mut attr = QueuePairAttribute::new();
        let mtu = remote_data.mtu;

        attr.setup_state(QueuePairState::ReadyToReceive)
            .setup_path_mtu(mtu)
            .setup_dest_qp_num(remote_data.qp_number)
            .setup_rq_psn(remote_data.psn)
            .setup_max_dest_read_atomic(1)
            .setup_min_rnr_timer(12);

        let mut ah_attr = AddressHandleAttribute::new();
        ah_attr
            .setup_port(1)
            .setup_grh_src_gid_index(local_data.gid_index)
            .setup_grh_dest_gid(&gid)
            .setup_grh_hop_limit(255);

        attr.setup_address_vector(&ah_attr);

        qp.modify(&attr).map_err(|e| {
            ConnectionError::RdmaError(format!("Failed to move QP to RTR: {:?}", e))
        })?;

        // Transition QP to RTS state
        let mut attr = QueuePairAttribute::new();
        attr.setup_state(QueuePairState::ReadyToSend)
            .setup_sq_psn(local_data.psn)
            .setup_timeout(14) // 4.096 μs * 2^14 = ~67 ms
            .setup_retry_cnt(7)
            .setup_rnr_retry(7)
            .setup_max_read_atomic(1);

        qp.modify(&attr).map_err(|e| {
            ConnectionError::RdmaError(format!("Failed to move QP to RTS: {:?}", e))
        })?;

        // Return the remote data
        Ok(remote_data)
    }

    fn send_raw(&self, _message_type: u32, payload: &[u8]) -> ConnectionResult<()> {
        let stream = self.stream.as_ref().ok_or_else(|| {
            ConnectionError::InvalidConfiguration("No active TCP connection".to_string())
        })?;

        let mut guard = stream.lock().unwrap();

        // Send the size
        let size = payload.len() as u64;
        guard
            .write_all(&size.to_le_bytes())
            .map_err(ConnectionError::IoError)?;

        // Send the payload
        guard.write_all(payload).map_err(ConnectionError::IoError)?;

        guard.flush().map_err(ConnectionError::IoError)?;
        Ok(())
    }

    fn receive_raw(&self) -> ConnectionResult<Vec<u8>> {
        let stream = self.stream.as_ref().ok_or_else(|| {
            ConnectionError::InvalidConfiguration("No active TCP connection".to_string())
        })?;

        let mut guard = stream.lock().unwrap();

        // Read the size
        let mut size_buffer = [0u8; 8];
        guard
            .read_exact(&mut size_buffer)
            .map_err(ConnectionError::IoError)?;
        let size = u64::from_le_bytes(size_buffer) as usize;

        // Read the payload
        let mut buffer = vec![0u8; size];
        guard
            .read_exact(&mut buffer)
            .map_err(ConnectionError::IoError)?;

        Ok(buffer)
    }
}

// TCP-specific dispatcher
pub struct TcpDispatcher {
    listener: TcpListener,
    _connections: HashMap<ConnectionId, ConnectionInfo>,
    _exchange_buffer: Vec<u8>,
    next_conn_id: AtomicU64,
    _next_worker: AtomicUsize,
}

// Implementation for TCP Dispatcher
impl TcpDispatcher {
    pub fn new(addr: SocketAddr, _worker_count: usize) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(addr)?;

        Ok(Self {
            listener,
            _connections: HashMap::new(),
            _exchange_buffer: vec![0; 1024],
            next_conn_id: AtomicU64::new(0),
            _next_worker: AtomicUsize::new(0),
        })
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        loop {
            let (_stream, _addr) = self.listener.accept()?;
            let _conn_id = ConnectionId(self.next_conn_id.fetch_add(1, Ordering::Relaxed));
        }
    }
}
