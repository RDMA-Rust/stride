use crate::cli::plan::Plan;
use crate::memory::aligned::DEFAULT_CACHE_LINE_SIZE;
use crate::memory::MemoryOps;
use anyhow::Result;
use sideway::ibverbs::completion::{
    CreateCompletionQueueWorkCompletionFlags, GenericCompletionQueue,
};
use sideway::ibverbs::device_context::DeviceContext;
use sideway::ibverbs::memory_region::MemoryRegion;
use sideway::ibverbs::protection_domain::ProtectionDomain;
use sideway::ibverbs::queue_pair::GenericQueuePair;
use std::sync::Arc;
use tracing::{debug, info};

const SEND_RECV_REGION_MULTIPLIER: usize = 2;

pub fn calculate_qp_region_offset(
    worker_offset: usize,
    increment_size: usize,
    qp_idx: usize,
) -> usize {
    worker_offset + qp_idx * increment_size * SEND_RECV_REGION_MULTIPLIER
}

pub fn compute_qp_slot_count(increment_size: usize) -> usize {
    ((increment_size / DEFAULT_CACHE_LINE_SIZE).max(1)).min(64)
}

pub fn calculate_send_slot_offset(increment_size: usize, buffer_slot: usize) -> usize {
    let slot_count = compute_qp_slot_count(increment_size);
    let slot_index = if slot_count > 0 {
        buffer_slot % slot_count
    } else {
        0
    };

    slot_index * DEFAULT_CACHE_LINE_SIZE
}

/// Calculate effective depth based on configured depth and total iterations
/// This prevents over-allocation when iteration count is smaller than configured depths
pub fn calculate_effective_depth(configured_depth: u32, total_iterations: u32) -> u32 {
    configured_depth.min(total_iterations).max(1) // Ensure at least 1
}

/// Worker that owns all RDMA resources needed for independent operation
/// This is the parent struct that has stable addresses for borrowing
pub struct Worker {
    /// Primary memory region for this worker (None for shared memory workers)
    pub memory_region: Option<Arc<MemoryRegion>>,
    /// Optional additional memory regions (for multi-buffer scenarios)
    pub additional_memory_regions: Vec<Arc<MemoryRegion>>,
    /// Memory allocator handle (for cleanup) - None for shared memory workers
    pub memory: Option<Box<dyn MemoryOps>>,
    /// Protection domain (shared across workers)
    pub pd: Arc<ProtectionDomain>,
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
    /// Maximum message size this worker can handle (for memory efficiency)
    pub max_msg_size: u32,
}

/// Worker context that contains all resources needed for a thread worker
/// to execute RDMA operations independently. With Arc-backed sideway handles we no
/// longer need the old `Rc<RefCell<_>>` wrappers, so the context stores and shares
/// completion queues directly while keeping deterministic drop ordering (QP before CQ).
pub struct WorkerContext {
    /// Queue pairs owned by this worker
    pub queue_pairs: Vec<GenericQueuePair>,
    /// Send completion queue shared across queue pairs
    pub send_completion_queue: GenericCompletionQueue,
    /// Receive completion queue shared across queue pairs (for SEND operations)
    pub recv_completion_queue: Option<GenericCompletionQueue>,
    /// Pre-calculated local buffer addresses flattened for cache efficiency
    /// Format: qp_buffer_addrs[qp_idx * tx_depth + operation_index] = local_addr
    pub qp_buffer_addrs: Vec<u64>,
    /// Pre-calculated remote buffer addresses flattened for cache efficiency
    /// Format: qp_remote_addrs[qp_idx * tx_depth + operation_index] = remote_addr
    pub qp_remote_addrs: Vec<u64>,
    /// TX depth for index calculations (cached for performance)
    pub tx_depth: u32,
    /// Maximum number of in-flight requests allowed globally (tx_depth * qp_count)
    pub max_inflight_requests: u32,
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
    pub iterations: u32,
    pub round: u32,
}

impl Worker {
    /// Create a new worker with all RDMA resources (legacy method)
    pub fn new(
        device: Arc<DeviceContext>,
        pd: Arc<ProtectionDomain>,
        memory_region: Arc<MemoryRegion>,
        memory: Box<dyn MemoryOps>,
        plan: Plan,
        thread_id: usize,
        tx_depth: u32,
        rx_depth: Option<u32>,
    ) -> Self {
        // Calculate effective depths based on plan iterations
        let total_iterations = plan.base().iters;
        let effective_tx_depth = calculate_effective_depth(tx_depth, total_iterations);
        let effective_rx_depth =
            rx_depth.map(|depth| calculate_effective_depth(depth, total_iterations));

        // Calculate maximum message size from plan
        let max_msg_size = plan.base().msg_sizes.iter().max().copied().unwrap_or(65536);

        info!(
            thread_id = thread_id,
            configured_tx_depth = tx_depth,
            effective_tx_depth = effective_tx_depth,
            configured_rx_depth = rx_depth,
            effective_rx_depth = effective_rx_depth,
            total_iterations = total_iterations,
            "Creating worker with dedicated memory and iteration-aware depths"
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
            tx_depth: effective_tx_depth,
            rx_depth: effective_rx_depth,
            base_addr,
            increment_size,
            worker_offset: 0,
            lkey,
            max_msg_size,
        }
    }

    /// Create a new worker with shared memory region (perftest-style)
    pub fn new_with_shared_memory(
        device: Arc<DeviceContext>,
        pd: Arc<ProtectionDomain>,
        lkey: u32, // Just pass the lkey, not the full MR
        base_addr: *mut u8,
        increment_size: usize,
        worker_offset: usize,
        plan: Plan,
        thread_id: usize,
        tx_depth: u32,
        rx_depth: Option<u32>,
    ) -> Self {
        // Calculate effective depths based on plan iterations
        let total_iterations = plan.base().iters;
        let effective_tx_depth = calculate_effective_depth(tx_depth, total_iterations);
        let effective_rx_depth =
            rx_depth.map(|depth| calculate_effective_depth(depth, total_iterations));

        // Calculate maximum message size from plan
        let max_msg_size = plan.base().msg_sizes.iter().max().copied().unwrap_or(65536);

        info!(
            thread_id = thread_id,
            worker_offset = worker_offset,
            increment_size = increment_size,
            lkey = lkey,
            configured_tx_depth = tx_depth,
            effective_tx_depth = effective_tx_depth,
            configured_rx_depth = rx_depth,
            effective_rx_depth = effective_rx_depth,
            total_iterations = total_iterations,
            "Creating worker with shared memory and iteration-aware depths"
        );

        Self {
            device,
            pd,
            memory_region: None, // No individual MR ownership for shared workers
            additional_memory_regions: Vec::new(),
            memory: None, // No individual memory ownership for shared workers
            plan,
            thread_id,
            tx_depth: effective_tx_depth,
            rx_depth: effective_rx_depth,
            base_addr,
            increment_size,
            worker_offset,
            lkey,
            max_msg_size,
        }
    }

    /// Calculate base address of the send buffer region for a specific QP
    #[inline(always)]
    pub fn calculate_qp_base_addr(&self, qp_idx: usize) -> u64 {
        let qp_offset = calculate_qp_region_offset(self.worker_offset, self.increment_size, qp_idx);
        unsafe { self.base_addr.add(qp_offset) as u64 }
    }

    /// Calculate address for a specific buffer slot (send region) within a QP
    #[inline(always)]
    pub fn calculate_operation_addr(&self, qp_idx: usize, buffer_slot: usize) -> u64 {
        let base_addr = self.calculate_qp_base_addr(qp_idx);

        // For send buffers, cycle through cache-line sized slots to mimic perftest pattern
        let offset = calculate_send_slot_offset(self.increment_size, buffer_slot);

        base_addr + offset as u64
    }

    /// Calculate message-size-aware address for incremental memory usage
    /// This allows using different portions of reserved memory based on actual message size
    #[inline(always)]
    pub fn calculate_message_size_addr(
        &self,
        qp_idx: usize,
        operation_index: u32,
        msg_size: u32,
        max_msg_size: u32,
    ) -> u64 {
        // Use the standard address calculation but adjust for message size efficiency
        let base_addr = self.calculate_operation_addr(qp_idx, operation_index as usize);

        // For messages smaller than max size, we can optimize memory usage
        // by using only the portion of memory we actually need
        if msg_size < max_msg_size && msg_size > 0 {
            let slot_count = compute_qp_slot_count(self.increment_size).min(16);
            let slot_index = if slot_count > 0 {
                operation_index as usize % slot_count
            } else {
                0
            };

            // Scale the stride based on actual message size while staying within send region
            let slots_per_message = ((msg_size as usize + DEFAULT_CACHE_LINE_SIZE - 1)
                / DEFAULT_CACHE_LINE_SIZE)
                .max(1);
            let mut offset = slot_index * slots_per_message * DEFAULT_CACHE_LINE_SIZE;
            let qp_offset =
                calculate_qp_region_offset(self.worker_offset, self.increment_size, qp_idx);

            let max_send_offset = self.increment_size.saturating_sub(DEFAULT_CACHE_LINE_SIZE);
            if offset > max_send_offset {
                offset = max_send_offset;
            }

            unsafe { self.base_addr.add(qp_offset + offset) as u64 }
        } else {
            // For max-sized messages or when sizes are equal, use standard addressing
            base_addr
        }
    }

    /// Total number of cache line slots available within the QP send region
    #[inline(always)]
    pub(crate) fn _qp_slot_count(&self) -> usize {
        compute_qp_slot_count(self.increment_size)
    }

    /// Add an additional memory region
    pub fn add_memory_region(&mut self, mr: Arc<MemoryRegion>) {
        debug!(
            thread_id = self.thread_id,
            mr_count = self.additional_memory_regions.len() + 1,
            "Adding additional memory region to worker"
        );
        self.additional_memory_regions.push(mr);
    }

    /// Get a reference to the primary memory region (for legacy compatibility)
    pub fn primary_memory_region(&self) -> Option<&MemoryRegion> {
        self.memory_region.as_deref()
    }

    /// Get the lkey for RDMA operations
    pub fn lkey(&self) -> u32 {
        self.lkey
    }

    /// Get all memory regions (primary + additional)
    pub fn all_memory_regions(&self) -> impl Iterator<Item = &Arc<MemoryRegion>> {
        self.memory_region
            .iter()
            .chain(self.additional_memory_regions.iter())
    }
}

impl WorkerContext {
    /// Create a new worker context using Arc-backed sideway resources.
    /// Completion queues are shared directly and all QP bookkeeping is prepared upfront.
    pub fn new(worker: &Worker, plan: &Plan, iterations: u32, qp_count: usize) -> Result<Self> {
        info!(
            thread_id = worker.thread_id,
            iterations = iterations,
            qp_count = qp_count,
            "Creating worker context"
        );

        // Create the send completion queue using the worker's device
        let send_cqe_size = worker.tx_depth * qp_count as u32;

        let send_cq: GenericCompletionQueue = worker
            .device
            .create_cq_builder()
            .setup_wc_flags(CreateCompletionQueueWorkCompletionFlags::StandardFlags)
            .setup_cqe(send_cqe_size)
            .build_ex()
            .map_err(|e| anyhow::anyhow!("Failed to create send CQ: {}", e))?
            .into();

        // For SEND operations, create a separate receive completion queue
        let recv_cq = if matches!(plan, crate::cli::plan::Plan::Send(_)) {
            let rx_depth = worker.rx_depth.unwrap_or(512);
            let recv_cqe_size = rx_depth * qp_count as u32;

            info!(
                thread_id = worker.thread_id,
                recv_cqe_size = recv_cqe_size,
                rx_depth = rx_depth,
                qp_count = qp_count,
                "Creating separate receive CQ for SEND operations"
            );

            let recv_cq: GenericCompletionQueue = worker
                .device
                .create_cq_builder()
                .setup_wc_flags(CreateCompletionQueueWorkCompletionFlags::StandardFlags)
                .setup_cqe(recv_cqe_size)
                .build_ex()
                .map_err(|e| anyhow::anyhow!("Failed to create recv CQ: {}", e))?
                .into();

            Some(recv_cq)
        } else {
            None
        };

        let tx_depth = worker.tx_depth;
        let max_inflight_requests = tx_depth.saturating_mul(qp_count as u32);

        Ok(Self {
            send_completion_queue: send_cq,
            recv_completion_queue: recv_cq,
            queue_pairs: Vec::new(),
            qp_buffer_addrs: Vec::new(),
            qp_remote_addrs: Vec::new(),
            tx_depth,
            max_inflight_requests,
            qp_send_counts: Vec::new(),
            qp_completion_counts: Vec::new(),
            total_requests: iterations * qp_count as u32,
            completed_requests: 0,
            inflight_requests: 0,
            iterations,
            round: 0,
        })
    }

    /// Add a queue pair to this worker context using unsafe code for lifetime management
    pub fn add_queue_pair(&mut self, worker: &Worker) -> Result<()> {
        // Create the queue pair using the worker's protection domain
        let recv_cq = self
            .recv_completion_queue
            .clone()
            .unwrap_or_else(|| self.send_completion_queue.clone());

        let qp = worker
            .pd
            .create_qp_builder()
            .setup_max_inline_data(256)
            .setup_send_cq(self.send_completion_queue.clone())
            .setup_recv_cq(recv_cq)
            .setup_max_send_wr(worker.tx_depth)
            .setup_max_recv_wr(worker.rx_depth.unwrap_or(512))
            .build_ex()
            .map_err(|e| anyhow::anyhow!("Failed to create QP: {}", e))?
            .into();

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

        let qp_idx = self.queue_pairs.len();
        for op_idx in 0..worker.tx_depth {
            let local_addr = worker.calculate_operation_addr(qp_idx, op_idx as usize);
            self.qp_buffer_addrs.push(local_addr);

            let local_offset = local_addr - (worker.base_addr as u64);
            self.qp_remote_addrs.push(local_offset);
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
    pub fn can_post_request(&self) -> bool {
        // Allow up to tx_depth outstanding requests per QP (perftest-style),
        // while still respecting total_requests.
        self.completed_requests + self.inflight_requests < self.total_requests
            && self.inflight_requests < self.max_inflight_requests
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
        self.inflight_requests = self.inflight_requests.saturating_sub(count);
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
        self.completed_requests += count;
        self.inflight_requests = self.inflight_requests.saturating_sub(count);
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
    pub fn get_queue_pair(&self, index: usize) -> Option<&GenericQueuePair> {
        self.queue_pairs.get(index)
    }

    /// Get mutable queue pair by index (bounds-checked)
    pub fn get_queue_pair_mut(&mut self, index: usize) -> Option<&mut GenericQueuePair> {
        self.queue_pairs.get_mut(index)
    }

    /// Get queue pair by index (unchecked in release builds).
    #[inline(always)]
    pub fn get_queue_pair_unchecked(&self, index: usize) -> &GenericQueuePair {
        debug_assert!(index < self.queue_pairs.len());
        unsafe { self.queue_pairs.get_unchecked(index) }
    }

    /// Get mutable queue pair by index (unchecked in release builds).
    #[inline(always)]
    pub fn get_queue_pair_mut_unchecked(&mut self, index: usize) -> &mut GenericQueuePair {
        debug_assert!(index < self.queue_pairs.len());
        unsafe { self.queue_pairs.get_unchecked_mut(index) }
    }

    /// Get the number of queue pairs
    pub fn queue_pair_count(&self) -> usize {
        self.queue_pairs.len()
    }

    /// Get send completion queue (for polling operations)
    pub fn completion_queue(&self) -> &GenericCompletionQueue {
        &self.send_completion_queue
    }

    /// Get send completion queue (explicit)
    pub fn send_completion_queue(&self) -> &GenericCompletionQueue {
        &self.send_completion_queue
    }

    /// Get receive completion queue (for SEND operations)
    pub fn recv_completion_queue(&self) -> Option<&GenericCompletionQueue> {
        self.recv_completion_queue.as_ref()
    }

    /// Update pre-calculated remote addresses with actual remote memory region base
    pub fn update_remote_addresses(&mut self, remote_mr_addr: u64) {
        for remote_addr in &mut self.qp_remote_addrs {
            *remote_addr += remote_mr_addr; // Convert offset to absolute address
        }
    }

    /// Post receive buffers for SEND operations (both client and server need this)
    pub fn post_receive_buffers(
        &mut self,
        worker: &Worker,
        rx_depth: u32,
        msg_size: u32,
    ) -> anyhow::Result<()> {
        use sideway::ibverbs::queue_pair::{QueuePair, SetScatterGatherEntry};

        for qp_idx in 0..self.queue_pair_count() {
            // Get QP (unchecked for performance)
            let qp = self.get_queue_pair_mut_unchecked(qp_idx);

            // Start post receive guard
            let mut guard = qp.start_post_recv();

            // Post rx_depth receive buffers for this QP
            for recv_idx in 0..rx_depth {
                // Use receive buffer area (second half of the memory region)
                // Each QP gets increment_size * 2 space: first half for send, second half for receive
                let recv_addr =
                    worker.calculate_qp_base_addr(qp_idx) + worker.increment_size as u64;
                let wr_id = (qp_idx as u64) << 32 | recv_idx as u64;

                // Create receive work request
                let recv_handle = guard.construct_wr(wr_id);

                // Setup scatter-gather entry for receive buffer
                unsafe {
                    recv_handle.setup_sge(worker.lkey(), recv_addr, msg_size);
                }
            }

            // Post all receive buffers for this QP
            guard
                .post()
                .map_err(|e| anyhow::anyhow!("Failed to post receive buffers: {}", e))?;
        }

        Ok(())
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
pub trait WorkerExecutor {
    /// Execute the worker's portion of the test
    fn execute(&mut self, context: &mut WorkerContext) -> Result<WorkerResult>;
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
