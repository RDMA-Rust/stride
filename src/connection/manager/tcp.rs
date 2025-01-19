use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use crate::connection::manager::ConnectionId;
use crate::connection::manager::ConnectionInfo;

// TCP-specific dispatcher
pub struct TcpDispatcher {
    listener: TcpListener,
    connections: HashMap<ConnectionId, ConnectionInfo>,
    exchange_buffer: Vec<u8>,
    next_conn_id: AtomicU64,
    next_worker: AtomicUsize,
}

// Implementation for TCP Dispatcher
impl TcpDispatcher {
    pub fn new(addr: SocketAddr, _worker_count: usize) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(addr)?;

        Ok(Self {
            listener,
            connections: HashMap::new(),
            exchange_buffer: vec![0; 1024],
            next_conn_id: AtomicU64::new(0),
            next_worker: AtomicUsize::new(0),
        })
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        loop {
            let (stream, addr) = self.listener.accept()?;
            let conn_id = ConnectionId(self.next_conn_id.fetch_add(1, Ordering::Relaxed));
        }
    }
}
