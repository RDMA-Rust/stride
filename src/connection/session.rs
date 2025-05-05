use serde::{Deserialize, Serialize};
use sideway::ibverbs::address::Gid;
use sideway::ibverbs::device_context::{DeviceContext, Mtu};
use sideway::ibverbs::protection_domain::ProtectionDomain;
use sideway::ibverbs::queue_pair::{GenericQueuePair, QueuePair};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::connection::exchange::{MemoryRegionInfo, TestResults};
use crate::connection::{
    ConnectionError, ConnectionFactory, ConnectionManager, ConnectionManagerExt, ConnectionParams,
    ConnectionResult, DestinationInfo, EndpointRole,
};
use crate::utils::random;

use tracing::{debug, info};

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
                info!("Listening on {}", addr);
                self.manager.accept()?;
                info!("Connection accepted from {}", self.manager.peer_addr()?);
            }
            EndpointRole::Client => {
                // Client mode - connect
                info!("Connecting to {}", addr);
                self.manager.connect(addr)?;
                info!("Connected to {}", self.manager.peer_addr()?);
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

        info!(
            local_addr = format!("0x{:x}", local_mr.addr),
            local_rkey = format!("0x{:x}", local_mr.rkey),
            local_size = format!("0x{:x}", local_mr.size),
            "Local MR information sent.",
        );

        info!(
            remote_addr = format!("0x{:x}", remote_mr.addr),
            remote_rkey = format!("0x{:x}", remote_mr.rkey),
            remote_size = format!("0x{:x}", remote_mr.size),
            "Remote MR information received.",
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

    /// Exchange test results with peer
    pub fn exchange_results(&self, results: TestResults) -> ConnectionResult<TestResults> {
        println!("Exchanging test results with peer");
        self.manager.exchange_results(results)
    }

    /// Receive test results (server mode)
    pub fn receive_results(&self) -> ConnectionResult<TestResults> {
        debug!("Waiting to receive test results from client");
        self.manager.receive_results()
    }

    /// Send test results (client mode)
    pub fn send_results(&self, results: &TestResults) -> ConnectionResult<()> {
        if results.bandwidth_result.is_some() {
            info!(
                test_type = format!("{:?}", results.test_type),
                message_size = results.size,
                iterations = results.iterations,
                bandwidth = format!(
                    "{:.5} Gbps",
                    results.bandwidth_result.as_ref().unwrap().bandwidth
                ),
                mpps = format!(
                    "{:.5} Mpps",
                    results.bandwidth_result.as_ref().unwrap().msg_rate
                ),
                "Sending bandwidth test results to server."
            );
        }
        if results.latency_result.is_some() {
            info!(
                test_type = format!("{:?}", results.test_type),
                message_size = results.size,
                iterations = results.iterations,
                avg_latency = format!(
                    "{:.5} us",
                    results.latency_result.as_ref().unwrap().avg_latency
                ),
                min_latency = format!(
                    "{:.5} us",
                    results.latency_result.as_ref().unwrap().min_latency
                ),
                max_latency = format!(
                    "{:.5} us",
                    results.latency_result.as_ref().unwrap().max_latency
                ),
                p99_latency = format!(
                    "{:.5} us",
                    results.latency_result.as_ref().unwrap().p99_latency
                ),
                p999_latency = format!(
                    "{:.5} us",
                    results.latency_result.as_ref().unwrap().p999_latency
                ),
                "Sending latency test results to server."
            );
        }

        self.manager.send_results(results)
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
