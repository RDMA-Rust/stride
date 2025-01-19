use serde::{Deserialize, Serialize};
use sideway::ibverbs::address::{Gid, GidType};

#[derive(Deserialize, Serialize, Debug)]
struct DestinationInfo {
    lid: u32,
    qp_number: u32,
    packet_seq_number: u32,
    gid: Gid,
    gid_type: GidType,
    gid_index: u32,
}
