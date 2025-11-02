use sideway::ibverbs::address::Gid;
use sideway::ibverbs::device_context::{DeviceContext, Mtu};
use sideway::ibverbs::protection_domain::ProtectionDomain;
use sideway::ibverbs::queue_pair::{GenericQueuePair, QueuePair};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::connection::exchange::{MemoryRegionInfo, TestResults};
use crate::connection::threaded::{
    ConnectionData, ThreadedConnectionFactory, ThreadedConnectionManager,
};
use crate::connection::{
    ConnectionError, ConnectionParams, ConnectionResult, ConnectionType, DestinationInfo,
};
use crate::utils::{mtu, random};

use tracing::{debug, info};

/// Connection session that runs connection operations in a separate thread for better performance
pub struct ConnectionSession {
    manager: ThreadedConnectionManager,
    ctx: Arc<DeviceContext>,
    _pd: Arc<ProtectionDomain>,
    local_gid_index: u8,
    local_gid: Option<Gid>,
    remote_gid: Option<Gid>,
    requested_mtu: Mtu,
    negotiated_mtu: Option<Mtu>,
}

impl ConnectionSession {
    /// Create a new connection session
    pub fn new(
        conn_type: ConnectionType,
        ctx: Arc<DeviceContext>,
        pd: Arc<ProtectionDomain>,
        params: ConnectionParams,
        gid_index: u8,
        requested_mtu: Mtu,
    ) -> ConnectionResult<Self> {
        let manager = ThreadedConnectionFactory::create(conn_type, params)?;

        Ok(Self {
            manager,
            ctx,
            _pd: pd,
            local_gid_index: gid_index,
            local_gid: None,
            remote_gid: None,
            requested_mtu,
            negotiated_mtu: None,
        })
    }

    /// Initialize the connection session
    pub fn initialize(&mut self) -> ConnectionResult<()> {
        self.manager.init()?;

        // Query the local GID for this port and index
        let gid_entry = self
            .ctx
            .query_gid_ex(1, self.local_gid_index as u32)
            .map_err(|e| ConnectionError::RdmaError(format!("Failed to query GID: {:?}", e)))?;

        self.local_gid = Some(gid_entry.gid());
        info!("Initialized session with local GID: {:?}", self.local_gid);

        Ok(())
    }

    /// Establish connection (either listen+accept for server or connect for client)
    pub fn establish_connection(&mut self, address: &str) -> ConnectionResult<()> {
        let addr: SocketAddr = if self.manager.connection_data().is_none() {
            // Parse the address
            address.parse().map_err(|e| {
                ConnectionError::AddressResolution(format!("Invalid address '{}': {}", address, e))
            })?
        } else {
            return Err(ConnectionError::InvalidConfiguration(
                "Connection already established".to_string(),
            ));
        };

        // Determine if we're server or client based on connection params
        // This logic should match the original session logic
        let is_server = match self.manager.connection_data() {
            Some(_) => {
                return Err(ConnectionError::InvalidConfiguration(
                    "Already connected".to_string(),
                ))
            }
            None => {
                // For now, assume server mode if address is "0.0.0.0" or similar
                address == "0.0.0.0" || address.starts_with("0.0.0.0:")
            }
        };

        if is_server {
            info!("Establishing server connection on {}", addr);
            self.manager.listen(addr)?;
            self.manager.accept()?;
        } else {
            info!("Establishing client connection to {}", addr);
            self.manager.connect(addr)?;
        }

        Ok(())
    }

    /// Setup a queue pair with the remote peer
    pub fn setup_queue_pair(
        &mut self,
        qp: &mut GenericQueuePair,
    ) -> ConnectionResult<DestinationInfo> {
        // Create local destination info
        let local_psn = random::generate_psn();
        let local_gid = self.local_gid.ok_or_else(|| {
            ConnectionError::InvalidConfiguration("Local GID not available".to_string())
        })?;

        let local_data = DestinationInfo {
            qp_number: qp.qp_number(),
            psn: local_psn,
            lid: 0, // Will be filled if needed
            gid: local_gid,
            gid_index: self.local_gid_index,
            gid_type: self
                .ctx
                .query_gid_ex(1, self.local_gid_index as u32)
                .map_err(|e| {
                    ConnectionError::RdmaError(format!("Failed to query GID type: {:?}", e))
                })?
                .gid_type(),
            mtu: self.requested_mtu,
        };

        // Exchange QP information through the threaded connection
        let remote_data = self.manager.exchange_qp_info(local_data.clone())?;
        self.remote_gid = Some(remote_data.gid);

        // Negotiate MTU - for now just use the minimum of requested and remote
        let negotiated_mtu =
            if mtu::mtu_to_value(self.requested_mtu) <= mtu::mtu_to_value(remote_data.mtu) {
                self.requested_mtu
            } else {
                remote_data.mtu
            };
        self.negotiated_mtu = Some(negotiated_mtu);

        info!(
            "MTU negotiation: requested={:?}, remote={:?}, negotiated={:?}",
            self.requested_mtu, remote_data.mtu, negotiated_mtu
        );

        // Setup the queue pair using the existing TCP connection manager's setup_qp logic
        // For now, we'll do a simplified QP setup here, but in the future this could be
        // moved to the threaded connection manager as well

        // TODO: Move QP setup to threaded manager for better encapsulation
        self.setup_qp_states(qp, &local_data, &remote_data)?;

        Ok(remote_data)
    }

    /// Setup queue pair states (simplified version)
    fn setup_qp_states(
        &self,
        qp: &mut GenericQueuePair,
        local_data: &DestinationInfo,
        remote_data: &DestinationInfo,
    ) -> ConnectionResult<()> {
        use sideway::ibverbs::address::AddressHandleAttribute;
        use sideway::ibverbs::queue_pair::{QueuePairAttribute, QueuePairState};
        use sideway::ibverbs::AccessFlags;

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

        // Transition QP to RTR state
        let mut attr = QueuePairAttribute::new();
        let negotiated_mtu = self.negotiated_mtu.unwrap_or(remote_data.mtu);

        attr.setup_state(QueuePairState::ReadyToReceive)
            .setup_path_mtu(negotiated_mtu)
            .setup_dest_qp_num(remote_data.qp_number)
            .setup_rq_psn(remote_data.psn)
            .setup_max_dest_read_atomic(1)
            .setup_min_rnr_timer(12);

        let mut ah_attr = AddressHandleAttribute::new();
        ah_attr
            .setup_port(1)
            .setup_grh_src_gid_index(local_data.gid_index)
            .setup_grh_dest_gid(&remote_data.gid)
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

        Ok(())
    }

    /// Exchange memory regions with the remote peer
    pub fn exchange_memory_regions(
        &mut self,
        local_addr: u64,
        local_rkey: u32,
        local_length: usize,
    ) -> ConnectionResult<MemoryRegionInfo> {
        let local_mr = MemoryRegionInfo {
            addr: local_addr,
            rkey: local_rkey,
            size: local_length,
        };

        self.manager.exchange_memory_regions(local_mr)
    }

    /// Synchronize queue pairs with the remote peer
    pub fn synchronize_qps(&mut self) -> ConnectionResult<()> {
        // For TCP connections, we don't need special QP synchronization
        // This is mainly for RDMA CM where we need to synchronize the connection state
        Ok(())
    }

    /// Synchronize before each message size test in all-sizes mode
    /// This ensures both client and server are ready before starting each message size
    pub fn synchronize_message_size(&mut self, msg_size: u32) -> ConnectionResult<()> {
        debug!(msg_size = msg_size, "Synchronizing for message size test");

        // Use a simple approach: exchange a dummy DestinationInfo as a sync barrier
        // This ensures both peers are at the same point before starting each message size test
        let gid_entry = self
            .ctx
            .query_gid_ex(1, self.local_gid_index as u32)
            .map_err(|e| {
                ConnectionError::RdmaError(format!("Failed to query GID for sync: {:?}", e))
            })?;

        let sync_data = DestinationInfo {
            lid: msg_size, // Use lid field as message size marker for sync
            mtu: self.requested_mtu,
            qp_number: msg_size, // Use qp_number field as additional sync marker
            psn: msg_size,       // Use PSN field as message size marker for sync
            gid: self.local_gid.unwrap_or_default(),
            gid_type: gid_entry.gid_type(),
            gid_index: self.local_gid_index,
        };

        // Exchange sync data - this acts as a barrier ensuring both peers are ready
        let _remote_sync = self.manager.exchange_qp_info(sync_data)?;

        debug!(
            msg_size = msg_size,
            "Message size synchronization completed"
        );
        Ok(())
    }

    /// Send test results to the remote peer
    pub fn send_results(&mut self, results: &TestResults) -> ConnectionResult<()> {
        self.manager.send_results(results)
    }

    /// Receive test results from the remote peer
    pub fn receive_results(&mut self) -> ConnectionResult<TestResults> {
        self.manager.receive_results()
    }

    /// Get the negotiated MTU
    pub fn negotiated_mtu(&self) -> Option<Mtu> {
        self.negotiated_mtu
    }

    /// Get connection data (addresses, etc.)
    pub fn connection_data(&self) -> Option<&ConnectionData> {
        self.manager.connection_data()
    }

    /// Close the connection session
    pub fn close(&mut self) -> ConnectionResult<()> {
        self.manager.close()
    }

    /// Get local GID
    pub fn local_gid(&self) -> Option<Gid> {
        self.local_gid
    }

    /// Get remote GID
    pub fn remote_gid(&self) -> Option<Gid> {
        self.remote_gid
    }
}
