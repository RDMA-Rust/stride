//! Flow control mechanism for RDMA operations
//!
//! This module implements a credit-based flow control system similar to perftest.
//! It prevents the sender from overwhelming the receiver by tracking available
//! credits that represent receive buffers at the remote side.

use memmap2::{MmapMut, MmapOptions};
use sideway::ibverbs::memory_region::MemoryRegion;
use sideway::ibverbs::protection_domain::ProtectionDomain;
use sideway::ibverbs::queue_pair::{
    GenericQueuePair, PostSendGuard, QueuePair, SetScatterGatherEntry, WorkRequestFlags,
};
use sideway::ibverbs::AccessFlags;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tracing::debug;

/// Flow control state for managing sender credits
#[derive(Debug, Clone)]
pub struct FlowControl<'a> {
    /// Memory-mapped buffer for credit tracking
    credit_map: Arc<MmapMut>,

    /// Atomic view of the credit counter for thread-safe updates
    credit_counter: Arc<AtomicU32>,

    /// Memory region for the credit buffer
    credit_mr: Option<Arc<MemoryRegion<'a>>>,

    /// Initial credit value (typically rx_depth/3)
    initial_credit: u32,

    /// Credit update threshold
    credit_threshold: u32,

    /// Remote memory information for credit updates
    remote_info: Option<RemoteMemoryInfo>,
}

#[derive(Clone, Debug)]
pub struct RemoteMemoryInfo {
    pub addr: u64,
    pub rkey: u32,
}

impl<'a> FlowControl<'a> {
    /// Create a new flow control instance
    pub fn new(initial_credit: u32) -> anyhow::Result<Self> {
        // Allocate memory-mapped buffer for credit tracking
        let credit_map = MmapOptions::new()
            .len(std::mem::size_of::<u32>())
            .map_anon()?;

        // Initialize credit counter
        let credit_counter = Arc::new(AtomicU32::new(initial_credit));

        Ok(Self {
            credit_map: Arc::new(credit_map),
            credit_counter,
            credit_mr: None,
            initial_credit,
            credit_threshold: initial_credit / 3, // Update when used 1/3 of credits
            remote_info: None,
        })
    }

    /// Register the credit buffer with the protection domain
    pub fn register_mr(&mut self, pd: &'a ProtectionDomain<'a>) -> anyhow::Result<()> {
        let mr = unsafe {
            pd.reg_mr(
                self.credit_map.as_ptr() as usize,
                self.credit_map.len(),
                AccessFlags::LocalWrite | AccessFlags::RemoteWrite,
            )
        }?;

        self.credit_mr = Some(Arc::new(mr));
        Ok(())
    }

    /// Get the memory region key and address for exchange with the peer
    pub fn get_memory_info(&self) -> Option<(u32, u64)> {
        self.credit_mr
            .as_ref()
            .map(|mr| (mr.rkey(), self.credit_map.as_ptr() as u64))
    }

    /// Set the remote memory information for credit updates
    pub fn set_remote_info(&mut self, addr: u64, rkey: u32) {
        self.remote_info = Some(RemoteMemoryInfo { addr, rkey });
    }

    /// Check if we have enough credits to send
    pub fn has_credit(&self) -> bool {
        self.credit_counter.load(Ordering::Relaxed) > 0
    }

    /// Consume a credit when posting a send
    pub fn consume_credit(&self) {
        self.credit_counter.fetch_sub(1, Ordering::Relaxed);
    }

    /// Check if we should send a credit update
    pub fn should_update_credit(&self, completed: u32) -> bool {
        completed % self.credit_threshold == 0
    }

    /// Update credit counter based on completions
    pub fn update_credit(&self, completed: u32) {
        unsafe { *(self.credit_map.as_ptr() as *mut u32) = completed };
    }

    /// Post a credit update to remote peer
    pub fn post_credit_update(
        &self,
        qp: &mut GenericQueuePair<'_>,
        completed: u32,
    ) -> anyhow::Result<()> {
        if let Some(remote_info) = &self.remote_info {
            if let Some(mr) = &self.credit_mr {
                // Update local buffer with completion count
                self.update_credit(completed);

                // Post RDMA WRITE to update remote credit
                let mut send_guard = qp.start_post_send();

                let wr_id = (0u64 << 32) | 0xFFFFFFFFu64; // Special marker for credit updates
                let send_handle = send_guard
                    .construct_wr(wr_id, WorkRequestFlags::Signaled)
                    .setup_write(remote_info.rkey, remote_info.addr);

                unsafe {
                    send_handle.setup_sge(
                        mr.lkey(),
                        self.credit_map.as_ptr() as u64,
                        std::mem::size_of::<u32>() as u32,
                    );
                }

                send_guard.post()?;
                tracing::info!("Posted credit update: {}", completed);
            }
        }
        Ok(())
    }

    /// Check remote credit from the buffer
    pub fn check_remote_credit(&self) -> u32 {
        unsafe { *(self.credit_map.as_ptr() as *const u32) }
    }

    /// Update local credit counter based on remote update
    pub fn process_remote_update(&self) {
        let remote_completed = self.check_remote_credit();
        let current_credit = self.credit_counter.load(Ordering::Relaxed);
        let new_credit = self.initial_credit.saturating_sub(remote_completed);

        if new_credit > current_credit {
            self.credit_counter.store(new_credit, Ordering::Relaxed);
            debug!("Credit updated: {} -> {}", current_credit, new_credit);
        }
    }

    /// Reset credit counter for next test
    pub fn reset(&self) {
        self.credit_counter
            .store(self.initial_credit, Ordering::Relaxed);
        self.update_credit(0);
    }
}

/// Flow control handler for the receiving side
#[derive(Debug, Clone)]
pub struct FlowControlReceiver<'a> {
    /// Memory-mapped buffer for credit tracking
    credit_map: Arc<MmapMut>,

    /// Memory region for the credit buffer
    credit_mr: Option<Arc<MemoryRegion<'a>>>,

    /// Counter for messages processed
    processed_count: u32,

    /// Credit update threshold
    credit_threshold: u32,
}

impl<'a> FlowControlReceiver<'a> {
    /// Create a new flow control receiver
    pub fn new(rx_depth: u32) -> anyhow::Result<Self> {
        // Allocate memory-mapped buffer
        let credit_map = MmapOptions::new()
            .len(std::mem::size_of::<u32>())
            .map_anon()?;

        Ok(Self {
            credit_map: Arc::new(credit_map),
            credit_mr: None,
            processed_count: 0,
            credit_threshold: rx_depth / 3,
        })
    }

    /// Register the credit buffer with the protection domain
    pub fn register_mr(&mut self, pd: &'a ProtectionDomain<'a>) -> anyhow::Result<()> {
        let mr = unsafe {
            pd.reg_mr(
                self.credit_map.as_ptr() as usize,
                self.credit_map.len(),
                AccessFlags::LocalWrite | AccessFlags::RemoteWrite,
            )
        }?;

        self.credit_mr = Some(Arc::new(mr));
        Ok(())
    }

    /// Get the memory region key and address for exchange with the peer
    pub fn get_memory_info(&self) -> Option<(u32, u64)> {
        self.credit_mr
            .as_ref()
            .map(|mr| (mr.rkey(), self.credit_map.as_ptr() as u64))
    }

    /// Process a received message
    pub fn process_completion(&mut self) -> bool {
        self.processed_count += 1;
        let should_update = self.processed_count % self.credit_threshold == 0;

        if should_update {
            // Update local buffer with current processed count
            unsafe { *(self.credit_map.as_ptr() as *mut u32) = self.processed_count };
        }

        should_update
    }

    /// Get the current processed count
    pub fn get_processed_count(&self) -> u32 {
        self.processed_count
    }

    /// Check the credit value from the remote side
    pub fn get_remote_processed_count(&self) -> u32 {
        unsafe { *(self.credit_map.as_ptr() as *const u32) }
    }

    /// Reset for a new test
    pub fn reset(&mut self) {
        self.processed_count = 0;
        unsafe { *(self.credit_map.as_ptr() as *mut u32) = 0 };
    }
}
