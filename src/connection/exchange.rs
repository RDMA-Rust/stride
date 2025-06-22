use serde::{Deserialize, Serialize};
use sideway::ibverbs::address::{Gid, GidType};
use sideway::ibverbs::device_context::Mtu;

use crate::utils::display::{BandwidthResult, LatencyResult};

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
pub struct MtuNegotiationInfo {
    pub validated_mtu: Mtu,
}

impl MtuNegotiationInfo {
    pub fn new(validated_mtu: Mtu) -> Self {
        Self { validated_mtu }
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
pub struct MemoryRegionInfo {
    pub addr: u64,   // Remote memory base address
    pub rkey: u32,   // Remote key for accessing the memory
    pub size: usize, // Size of the memory region
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
pub enum TestType {
    Bandwidth,
    Latency,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TestResults {
    pub test_type: TestType,
    pub size: u32,
    pub iterations: u32,
    pub time: String,

    pub bandwidth_result: Option<BandwidthResult>,
    pub latency_result: Option<LatencyResult>,
}

pub struct ConnectionSetupResult {
    pub remote_mr: MemoryRegionInfo,
    pub gid_type: GidType,
    pub local_gid: Gid,
    pub remote_gid: Gid,
    pub actual_mtu: u32,
    pub qp_details: Vec<QueuePairConnection>,
}

#[derive(Debug, Clone)]
pub struct QueuePairConnection {
    pub local_qpn: u32,
    pub local_psn: u32,
    pub remote_qpn: u32,
    pub remote_psn: u32,
}
