use serde::{Deserialize, Serialize};
use sideway::ibverbs::address::{Gid, GidType};
use sideway::ibverbs::device_context::Mtu;

#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
pub struct DestinationInfo {
    pub lid: u32,
    pub mtu: Mtu,
    pub qp_number: u32,
    pub psn: u32,
    pub gid: Gid,
    pub gid_type: GidType,
    pub gid_index: u8,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
pub struct MemoryRegionInfo {
    pub addr: u64,   // Remote memory base address
    pub rkey: u32,   // Remote key for accessing the memory
    pub size: usize, // Size of the memory region
}
