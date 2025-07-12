use crate::cli::plan::Plan;
use crate::memory::MemoryOps;
use anyhow::Result;
use sideway::ibverbs::completion::{
    CreateCompletionQueueWorkCompletionFlags, ExtendedCompletionQueue,
};
use sideway::ibverbs::device_context::DeviceContext;
use sideway::ibverbs::memory_region::MemoryRegion;
use sideway::ibverbs::protection_domain::ProtectionDomain;
use sideway::ibverbs::queue_pair::GenericQueuePair;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use tracing::{debug, info};

/// Worker that owns all RDMA resources needed for independent operation
/// This is the parent struct that has stable addresses for borrowing
pub struct Worker<'a> {
    /// Primary memory region for this worker (None for shared memory workers)
    pub memory_region: Option<MemoryRegion<'a>>,
    /// Optional additional memory regions (for multi-buffer scenarios)
    pub additional_memory_regions: Vec<MemoryRegion<'a>>,
    /// Memory allocator handle (for cleanup) - None for shared memory workers
    pub memory: Option<Box<dyn MemoryOps>>,
    /// Protection domain (shared across workers)
    pub pd: Arc<ProtectionDomain<'a>>,
    /// Device context (shared across workers)
    pub device: Arc<DeviceContext>,
    /// Test plan configuration
    pub plan: Plan,
    /// Worker thread identifier
    pub thread_id: usize,
    /// TX depth for queue pairs
    pub tx_depth: u32,
    /// RX depth for queue pairs
    pub rx_depth: Option<u32>,
    /// perftest-style address calculation fields
    pub base_addr: *mut u8, // Base address for this worker's memory region
    pub increment_size: usize, // Cache-aligned increment size
    pub worker_offset: usize,  // Offset within shared buffer for this worker
    /// Memory region handle for shared memory workers
    pub lkey: u32, // Local key for RDMA operations
}

/// Worker context that contains all resources needed for a thread worker
/// to execute RDMA operations independently. Uses Rc<RefCell<>> for single-threaded
/// performance while handling self-referential lifetime issues safely.
///
/// Element orders matter, as we should release QP first and then release CQ
pub struct WorkerContext<'a> {
    /// Queue pairs owned by this worker
    pub queue_pairs: Vec<GenericQueuePair<'a>>,
    /// Completion queue wrapped in Rc<RefCell<>> for safe sharing
    pub completion_queue: Rc<RefCell<ExtendedCompletionQueue<'a>>>,
    /// Pre-calculated local buffer addresses flattened for cache efficiency
    /// Format: qp_buffer_addrs[qp_idx * tx_depth + operation_index] = local_addr
    pub qp_buffer_addrs: Vec<u64>,
    /// Pre-calculated remote buffer addresses flattened for cache efficiency
    /// Format: qp_remote_addrs[qp_idx * tx_depth + operation_index] = remote_addr
    pub qp_remote_addrs: Vec<u64>,
    /// TX depth for index calculations (cached for performance)
    pub tx_depth: u32,
    /// Per-QP send counters (like perftest's scnt[])
    pub qp_send_counts: Vec<u32>,
    /// Per-QP completion counters (like perftest's ccnt[])
    pub qp_completion_counts: Vec<u32>,
    /// Total number of requests this worker should process
    pub total_requests: u32,
    /// Number of completed requests so far (global)
    pub completed_requests: u32,
    /// Number of requests currently in flight (global)
    pub inflight_requests: u32,
}

impl<'a> Worker<'a> {
    /// Create a new worker with all RDMA resources (legacy method)
    pub fn new(
        device: Arc<DeviceContext>,
        pd: Arc<ProtectionDomain<'a>>,
        memory_region: MemoryRegion<'a>,
        memory: Box<dyn MemoryOps>,
        plan: Plan,
        thread_id: usize,
        tx_depth: u32,
        rx_depth: Option<u32>,
    ) -> Self {
        info!(
            thread_id = thread_id,
            "Creating worker with dedicated memory"
        );

        // Calculate base address and default increment for legacy compatibility
        let base_addr = memory_region.get_ptr() as *mut u8;
        let increment_size = 64; // Default cache line size
        let lkey = memory_region.lkey();

        Self {
            device,
            pd,
            memory_region: Some(memory_region),
            additional_memory_regions: Vec::new(),
            memory: Some(memory),
            plan,
            thread_id,
            tx_depth,
            rx_depth,
            base_addr,
            increment_size,
            worker_offset: 0,
            lkey,
        }
    }

    /// Create a new worker with shared memory region (perftest-style)
    pub fn new_with_shared_memory(
        device: Arc<DeviceContext>,
        pd: Arc<ProtectionDomain<'a>>,
        lkey: u32, // Just pass the lkey, not the full MR
        base_addr: *mut u8,
        increment_size: usize,
        worker_offset: usize,
        plan: Plan,
        thread_id: usize,
        tx_depth: u32,
        rx_depth: Option<u32>,
    ) -> Self {
        info!(
            thread_id = thread_id,
            worker_offset = worker_offset,
            increment_size = increment_size,
            lkey = lkey,
            "Creating worker with shared memory"
        );

        Self {
            device,
            pd,
            memory_region: None, // No individual MR ownership for shared workers
            additional_memory_regions: Vec::new(),
            memory: None, // No individual memory ownership for shared workers
            plan,
            thread_id,
            tx_depth,
            rx_depth,
            base_addr,
            increment_size,
            worker_offset,
            lkey,
        }
    }

    /// Calculate address for a specific operation following perftest pattern exactly
    #[inline(always)]
    pub fn calculate_operation_addr(&self, operation_index: u32, _msg_size: u32) -> u64 {
        // PERFORMANCE CRITICAL: perftest-style address cycling for optimal cache behavior

        // perftest pattern: cycle within the worker's own memory space
        // operation_index is masked to tx_depth, so we cycle within our allocated space

        // Each operation gets a cache-line aligned offset within this worker's space
        // Use smaller cycling to stay within worker boundaries
        let cycle_mask = 0x3F; // Cycle through 64 positions (4KB for 64-byte cache lines)
        let addr_offset = ((operation_index as usize) & cycle_mask) * 64; // Cache line size
        let final_addr_offset = self.worker_offset + addr_offset;

        unsafe { self.base_addr.add(final_addr_offset) as u64 }
    }

    /// Add an additional memory region
    pub fn add_memory_region(&mut self, mr: MemoryRegion<'a>) {
        debug!(
            thread_id = self.thread_id,
            mr_count = self.additional_memory_regions.len() + 1,
            "Adding additional memory region to worker"
        );
        self.additional_memory_regions.push(mr);
    }

    /// Get the primary memory region (for legacy compatibility)
    pub fn primary_memory_region(&self) -> Option<&MemoryRegion<'a>> {
        self.memory_region.as_ref()
    }

    /// Get the lkey for RDMA operations
    pub fn lkey(&self) -> u32 {
        self.lkey
    }

    /// Get all memory regions (primary + additional)
    pub fn all_memory_regions(&self) -> impl Iterator<Item = &MemoryRegion<'a>> {
        self.memory_region
            .iter()
            .chain(self.additional_memory_regions.iter())
    }
}

impl<'a> WorkerContext<'a> {
    /// Create a new worker context using unsafe code to handle self-referential lifetimes
    /// This follows the pattern you suggested with Rc<RefCell<>> for the completion queue
    pub fn new(worker: &'a Worker<'a>, _plan: &Plan, total_requests: u32) -> Result<Self> {
        info!(
            thread_id = worker.thread_id,
            total_requests = total_requests,
            "Creating worker context"
        );

        // Create the completion queue using the worker's device
        let cq = worker
            .device
            .create_cq_builder()
            .setup_wc_flags(CreateCompletionQueueWorkCompletionFlags::StandardFlags)
            .setup_cqe(worker.tx_depth * 2) // Extra space for safety
            .build_ex()
            .map_err(|e| anyhow::anyhow!("Failed to create CQ: {}", e))?;

        // Wrap the CQ in Rc<RefCell<>> first
        let cq_wrapped = Rc::new(RefCell::new(cq));

        Ok(Self {
            completion_queue: cq_wrapped,
            queue_pairs: Vec::new(),
            qp_buffer_addrs: Vec::new(),
            qp_remote_addrs: Vec::new(),
            tx_depth: worker.tx_depth,
            qp_send_counts: Vec::new(),
            qp_completion_counts: Vec::new(),
            total_requests,
            completed_requests: 0,
            inflight_requests: 0,
        })
    }

    /// Add a queue pair to this worker context using unsafe code for lifetime management
    pub fn add_queue_pair(&mut self, worker: &'a Worker<'a>) -> Result<()> {
        // Create the queue pair using the worker's protection domain
        // We need to use unsafe to get a raw reference that lives long enough
        let qp = unsafe {
            let cq_ptr = self.completion_queue.as_ptr();
            let cq_ref = &*cq_ptr;

            worker
                .pd
                .create_qp_builder()
                .setup_max_inline_data(256)
                .setup_send_cq(cq_ref)
                .setup_recv_cq(cq_ref)
                .setup_max_send_wr(worker.tx_depth)
                .setup_max_recv_wr(worker.rx_depth.unwrap_or(512))
                .build_ex()
                .map_err(|e| anyhow::anyhow!("Failed to create QP: {}", e))?
                .into()
        };

        debug!(
            thread_id = worker.thread_id,
            qp_count = self.queue_pairs.len() + 1,
            "Adding queue pair to worker context"
        );

        // Pre-calculate buffer addresses for this QP to avoid hot-path calculations
        
        // Reserve space for this QP's addresses in the flattened vectors
        let tx_depth = worker.tx_depth as usize;
        self.qp_buffer_addrs.reserve(tx_depth);
        self.qp_remote_addrs.reserve(tx_depth);

        for op_idx in 0..worker.tx_depth {
            // Pre-calculate local address for this operation index
            let local_addr = worker.calculate_operation_addr(op_idx, 0); // msg_size not needed for addr calc
            self.qp_buffer_addrs.push(local_addr);

            // Pre-calculate remote address offset (will be updated with actual remote_mr later)
            let local_offset = local_addr - (worker.base_addr as u64);
            self.qp_remote_addrs.push(local_offset); // Store offset for now, will add remote_mr.addr later
        }
        self.qp_send_counts.push(0); // Initialize per-QP send counter
        self.qp_completion_counts.push(0); // Initialize per-QP completion counter
        self.queue_pairs.push(qp);
        Ok(())
    }

    /// Check if worker has completed all requests
    pub fn is_complete(&self) -> bool {
        self.completed_requests >= self.total_requests && self.inflight_requests == 0
    }

    /// Check if worker can post more requests (global check)
    pub fn can_post_request(&self, tx_depth: u32) -> bool {
        self.completed_requests + self.inflight_requests < self.total_requests
            && self.inflight_requests < tx_depth
    }

    /// Check if a specific QP can post more requests (perftest-style per-QP flow control)
    pub fn can_qp_post_request(&self, qp_idx: usize, tx_depth: u32) -> bool {
        // perftest pattern: scnt[qp] - ccnt[qp] < tx_depth
        let qp_inflight = self.qp_send_counts[qp_idx] - self.qp_completion_counts[qp_idx];
        qp_inflight < tx_depth
    }

    /// Record that a request was posted
    pub fn record_request_posted(&mut self) {
        self.inflight_requests += 1;
        debug!(
            inflight = self.inflight_requests,
            completed = self.completed_requests,
            total = self.total_requests,
            "Request posted"
        );
    }

    /// Record that multiple requests were posted (batch version for performance)
    #[inline(always)]
    pub fn record_requests_posted(&mut self, count: u32) {
        self.inflight_requests += count;
        debug!(
            inflight = self.inflight_requests,
            completed = self.completed_requests,
            total = self.total_requests,
            posted_count = count,
            "Batch requests posted"
        );
    }

    /// Record that a request was completed
    #[inline(always)]
    pub fn record_request_completed(&mut self) {
        if self.inflight_requests > 0 {
            self.inflight_requests -= 1;
        }
        self.completed_requests += 1;
        // Removed debug logging from hot path for performance
    }

    /// Record that multiple requests were completed (batch version for performance)
    #[inline(always)]
    pub fn record_requests_completed(&mut self, count: u32) {
        self.inflight_requests -= count;
        self.completed_requests += count;
        // Removed debug logging from hot path for performance
    }

    /// Record that requests were posted to a specific QP (perftest-style per-QP tracking)
    #[inline(always)]
    pub fn record_qp_requests_posted(&mut self, qp_idx: usize, count: u32) {
        self.qp_send_counts[qp_idx] += count;
        self.inflight_requests += count;
    }

    /// Record that requests were completed from a specific QP (perftest-style per-QP tracking)
    #[inline(always)]
    pub fn record_qp_requests_completed(&mut self, qp_idx: usize, count: u32) {
        self.qp_completion_counts[qp_idx] += count;
        self.inflight_requests = self.inflight_requests.saturating_sub(count);
        self.completed_requests += count;
    }

    /// Get progress as a percentage
    pub fn progress_percentage(&self) -> f64 {
        if self.total_requests == 0 {
            100.0
        } else {
            (self.completed_requests as f64 / self.total_requests as f64) * 100.0
        }
    }

    /// Get queue pair by index (bounds-checked)
    pub fn get_queue_pair(&self, index: usize) -> Option<&GenericQueuePair<'a>> {
        self.queue_pairs.get(index)
    }

    /// Get mutable queue pair by index (bounds-checked)
    pub fn get_queue_pair_mut(&mut self, index: usize) -> Option<&mut GenericQueuePair<'a>> {
        self.queue_pairs.get_mut(index)
    }

    /// Get queue pair by index (unchecked for hot paths)
    /// SAFETY: Caller must ensure index < queue_pair_count()
    #[inline(always)]
    pub unsafe fn get_queue_pair_unchecked(&self, index: usize) -> &GenericQueuePair<'a> {
        self.queue_pairs.get_unchecked(index)
    }

    /// Get mutable queue pair by index (unchecked for hot paths)
    /// SAFETY: Caller must ensure index < queue_pair_count()
    #[inline(always)]
    pub unsafe fn get_queue_pair_mut_unchecked(
        &mut self,
        index: usize,
    ) -> &mut GenericQueuePair<'a> {
        self.queue_pairs.get_unchecked_mut(index)
    }

    /// Get the number of queue pairs
    pub fn queue_pair_count(&self) -> usize {
        self.queue_pairs.len()
    }

    /// Get completion queue (for polling operations)
    pub fn completion_queue(&self) -> &Rc<RefCell<ExtendedCompletionQueue<'a>>> {
        &self.completion_queue
    }

    /// Update pre-calculated remote addresses with actual remote memory region base
    pub fn update_remote_addresses(&mut self, remote_mr_addr: u64) {
        for remote_addr in &mut self.qp_remote_addrs {
            *remote_addr += remote_mr_addr; // Convert offset to absolute address
        }
    }

    /// Get pre-calculated local address for QP and operation index (hot path optimized)
    #[inline(always)]
    pub fn get_local_addr(&self, qp_idx: usize, operation_index: u32) -> u64 {
        // PERFORMANCE CRITICAL: Flattened array access, single indirection
        // SAFETY: Caller must ensure qp_idx < queue_pair_count() and operation_index < tx_depth
        let flat_index = qp_idx * self.tx_depth as usize + operation_index as usize;
        unsafe { *self.qp_buffer_addrs.get_unchecked(flat_index) }
    }

    /// Get pre-calculated remote address for QP and operation index (hot path optimized)
    #[inline(always)]
    pub fn get_remote_addr(&self, qp_idx: usize, operation_index: u32) -> u64 {
        // PERFORMANCE CRITICAL: Flattened array access, single indirection
        // SAFETY: Caller must ensure qp_idx < queue_pair_count() and operation_index < tx_depth
        let flat_index = qp_idx * self.tx_depth as usize + operation_index as usize;
        unsafe { *self.qp_remote_addrs.get_unchecked(flat_index) }
    }
}

/// Trait for worker execution strategies
pub trait WorkerExecutor<'a> {
    /// Execute the worker's portion of the test
    fn execute(&mut self, context: &mut WorkerContext<'a>) -> Result<WorkerResult>;
}

/// Result from a worker execution
#[derive(Debug, Clone)]
pub struct WorkerResult {
    pub thread_id: usize,
    pub completed_requests: u32,
    pub total_requests: u32,
    pub execution_time_ns: u64,
    pub latency_samples: Vec<u64>, // For latency tests
    pub error_count: u32,
}

impl WorkerResult {
    pub fn new(thread_id: usize, total_requests: u32) -> Self {
        Self {
            thread_id,
            completed_requests: 0,
            total_requests,
            execution_time_ns: 0,
            latency_samples: Vec::new(),
            error_count: 0,
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            100.0
        } else {
            ((self.completed_requests - self.error_count) as f64 / self.total_requests as f64)
                * 100.0
        }
    }
}

// Factory methods will be implemented later when we understand the usage patterns better
// For now, users should create Worker instances directly using Worker::new()

// Legacy function for compatibility
use crate::runners::runner::PlanTestRunner;

pub fn run_worker(plan: Plan) -> Result<()> {
    let runner = PlanTestRunner::new(plan);
    runner.run().map_err(|e| anyhow::anyhow!("{}", e))
}
