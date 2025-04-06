use serde::{Deserialize, Serialize};
use sideway::ibverbs::address::Gid;
use sideway::ibverbs::device_context::{DeviceContext, Mtu};
use sideway::ibverbs::protection_domain::ProtectionDomain;
use sideway::ibverbs::queue_pair::{GenericQueuePair, QueuePair};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::connection::exchange::MemoryRegionInfo;
use crate::connection::{
    ConnectionError, ConnectionFactory, ConnectionManager, ConnectionManagerExt, ConnectionParams,
    ConnectionResult, DestinationInfo, EndpointRole,
};
use crate::utils::random;

pub struct ConnectionSession<'a> {
    manager: Box<dyn ConnectionManager>,
    ctx: Arc<DeviceContext>,
    pd: Arc<ProtectionDomain<'a>>,
    local_gid_index: u8,
    local_gid: Option<Gid>,
    remote_gid: Option<Gid>,
}

impl<'a> ConnectionSession<'a> {
    pub fn new(
        conn_type: &str,
        ctx: Arc<DeviceContext>,
        pd: Arc<ProtectionDomain<'a>>,
        params: ConnectionParams,
        gid_index: u8,
    ) -> ConnectionResult<Self> {
        let manager = ConnectionFactory::create(conn_type, params)?;

        Ok(Self {
            manager,
            ctx,
            pd,
            local_gid_index: gid_index,
            local_gid: None,
            remote_gid: None,
        })
    }

    pub fn initialize(&mut self) -> ConnectionResult<()> {
        self.manager.init()
    }

    pub fn establish_connection(&mut self, address: &str) -> ConnectionResult<()> {
        let addr: SocketAddr = address.parse().map_err(|e: std::net::AddrParseError| {
            ConnectionError::AddressResolution(e.to_string())
        })?;

        match self.manager.params().role {
            EndpointRole::Server => {
                // Server mode - bind and accept
                self.manager.listen(addr)?;
                println!("Listening on {}", addr);
                self.manager.accept()?;
                println!("Connection accepted from {}", self.manager.peer_addr()?);
            }
            EndpointRole::Client => {
                // Client mode - connect
                println!("Connecting to {}", addr);
                self.manager.connect(addr)?;
                println!("Connected to {}", self.manager.peer_addr()?);
            }
        }

        Ok(())
    }

    pub fn exchange_memory_regions(
        &self,
        mr_addr: u64,
        rkey: u32,
        size: usize,
    ) -> ConnectionResult<MemoryRegionInfo> {
        // Create local MR info
        let local_mr = MemoryRegionInfo {
            addr: mr_addr,
            rkey,
            size,
        };

        // Exchange with remote peer
        let remote_mr = self.manager.exchange_memory_regions(local_mr)?;

        println!("Memory regions exchanged:");
        println!(
            "  Local: addr=0x{:x}, rkey=0x{:x}, size={}",
            local_mr.addr, local_mr.rkey, local_mr.size
        );
        println!(
            "  Remote: addr=0x{:x}, rkey=0x{:x}, size={}",
            remote_mr.addr, remote_mr.rkey, remote_mr.size
        );

        Ok(remote_mr)
    }

    pub fn setup_queue_pair(
        &mut self,
        qp: &mut GenericQueuePair<'_>,
    ) -> ConnectionResult<DestinationInfo> {
        // Prepare local QP data
        let local_data = self.prepare_local_qp_data(qp)?;
        self.local_gid = Some(local_data.gid);

        // Setup QP with remote data
        let remote_data = self.manager.setup_qp(&self.ctx, &self.pd, qp, local_data)?;
        self.remote_gid = Some(remote_data.gid);

        Ok(remote_data)
    }

    pub fn synchronize_qps(&self) -> ConnectionResult<()> {
        // Simple ready message
        #[derive(Serialize, Deserialize)]
        struct ReadyMessage {
            ready: bool,
        }

        // This works because of the extension trait
        self.manager
            .send_message(2, &ReadyMessage { ready: true })?;
        let remote_ready: ReadyMessage = self.manager.receive_message()?;

        if !remote_ready.ready {
            return Err(ConnectionError::ExchangeFailed(
                "Remote side not ready".to_string(),
            ));
        }

        Ok(())
    }

    fn prepare_local_qp_data(
        &self,
        qp: &GenericQueuePair<'_>,
    ) -> ConnectionResult<DestinationInfo> {
        // Get local GID information
        let gid = self
            .ctx
            .query_gid_ex(1, self.local_gid_index as u32)
            .map_err(|e| ConnectionError::RdmaError(format!("Failed to query GID: {:?}", e)))?;

        // Generate random PSN
        let psn = random::generate_psn();

        Ok(DestinationInfo {
            qp_number: qp.qp_number(),
            lid: 0, // For RoCE, LID is typically 0
            gid: gid.gid(),
            gid_type: gid.gid_type(),
            gid_index: self.local_gid_index,
            psn,
            mtu: Mtu::Mtu4096,
        })
    }

    pub fn close(&mut self) -> ConnectionResult<()> {
        self.manager.close()
    }
}
