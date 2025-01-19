use serde::{Deserialize, Serialize};
use sideway::ibverbs::address::Gid;
use sideway::ibverbs::device_context::Mtu;
use std::net::SocketAddr;

pub mod rdmacm;
pub mod tcp;

// Common types for both TCP and RDMACM
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct ConnectionId(u64);

#[derive(Debug)]
pub struct ConnectionInfo {
    pub conn_id: ConnectionId,
    pub local_addr: SocketAddr,
    pub peer_addr: SocketAddr,
    pub qp_info: QueuePairInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueuePairInfo {
    pub qp_num: u32,
    pub lid: u16,
    pub gid: Gid,
    pub psn: u32,
    pub mtu: Mtu,
}
