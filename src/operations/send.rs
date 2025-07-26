use crate::connection::message;
use crate::runners::worker::{Worker, WorkerContext};
use anyhow::Result;
use quanta::IntoNanoseconds;
use sideway::ibverbs::completion::WorkCompletionStatus;
use sideway::ibverbs::queue_pair::{
    PostSendGuard, QueuePair, SetScatterGatherEntry, WorkRequestFlags,
};
use std::cell::RefCell;
use std::rc::Rc;
use tracing::{debug, info};

/// Credit-based flow control for SEND operations
/// Based on perftest's flow control implementation
#[derive(Debug)]
pub struct SendFlowControl {
    /// Send credits per QP (like perftest's scredit_for_qp)
    pub send_credits: Vec<i32>,
    /// Receive credits per QP to track posted receive buffers
    pub recv_credits: Vec<u32>,
    /// Total posted receive buffers per QP (lifetime accumulation)
    pub posted_recv_per_qp: Vec<u32>,
    /// Per-QP message size tracking for posted buffers
    pub qp_recv_msg_size: Vec<u32>,
    /// Per-QP buffer count for current message size
    pub qp_recv_buffers_for_msg_size: Vec<u32>,
    /// Maximum send credits (typically rx_depth)
    pub max_send_credits: i32,
    /// RX depth for receive buffer management
    pub rx_depth: u32,
    /// Global tracking for message size transitions (for debugging)
    pub current_recv_msg_size: u32,
    /// How many receive buffers we've posted for current message size across all QPs
    pub posted_recv_for_current_size: u32,
}

impl SendFlowControl {
    /// Create new flow control manager
    pub fn new(
        qp_count: usize,
        rx_depth: u32,
        per_qp_iterations: u32,
        total_iterations: u32,
        message_sizes: Vec<u32>,
    ) -> Self {
        // RX depth should be minimum of configured depth and total iterations
        let effective_rx_depth = rx_depth.min(per_qp_iterations);
        let max_send_credits = effective_rx_depth as i32;

        Self {
            send_credits: vec![max_send_credits; qp_count],
            recv_credits: vec![0; qp_count], // Start with 0 receive buffers posted
            posted_recv_per_qp: vec![0; qp_count],
            qp_recv_msg_size: vec![0; qp_count], // Track message size per QP
            qp_recv_buffers_for_msg_size: vec![0; qp_count], // Buffer count per QP per message size
            max_send_credits,
            rx_depth: effective_rx_depth,
            current_recv_msg_size: 0, // Global tracking for debugging
            posted_recv_for_current_size: 0,
        }
    }

    /// Check if QP can send (has send credits)
    pub fn can_send(&self, qp_idx: usize) -> bool {
        self.send_credits[qp_idx] > 0
    }

    /// Consume send credits for posting operations
    pub fn consume_send_credits(&mut self, qp_idx: usize, count: u32) {
        self.send_credits[qp_idx] -= count as i32;
        debug!(
            qp_idx = qp_idx,
            consumed = count,
            remaining = self.send_credits[qp_idx],
            "Consumed send credits"
        );
    }

    /// Restore send credits when operations complete
    pub fn restore_send_credits(&mut self, qp_idx: usize, count: u32) {
        self.send_credits[qp_idx] += count as i32;
        debug!(
            qp_idx = qp_idx,
            restored = count,
            total = self.send_credits[qp_idx],
            "Restored send credits"
        );
    }

    /// Check if QP needs more receive buffers
    pub fn needs_recv_buffers(&self, qp_idx: usize) -> bool {
        // debug!(qp_idx = qp_idx, recv_credits = self.recv_credits[qp_idx], rx_depth = self.rx_depth);
        self.recv_credits[qp_idx] < self.rx_depth // Always try to keep receive queue full
    }

    /// Check if we need to post receive buffers for a new message size
    pub fn needs_recv_buffers_for_msg_size(&self, qp_idx: usize, msg_size: u32) -> bool {
        // Check if this QP has buffers posted for this specific message size
        if self.qp_recv_msg_size[qp_idx] != msg_size {
            debug!(
                qp_idx = qp_idx,
                qp_current_msg_size = self.qp_recv_msg_size[qp_idx],
                requested_msg_size = msg_size,
                qp_recv_credits = self.recv_credits[qp_idx],
                "QP needs buffers for new message size"
            );
            return true;
        }

        // If same message size, check if this QP needs more buffers
        let needs_more = self.needs_recv_buffers(qp_idx);
        if needs_more {
            debug!(
                qp_idx = qp_idx,
                recv_credits = self.recv_credits[qp_idx],
                rx_depth = self.rx_depth,
                buffers_for_msg_size = self.qp_recv_buffers_for_msg_size[qp_idx],
                "QP needs more receive buffers for same message size"
            );
        }
        needs_more
    }

    /// Record receive buffer consumption
    pub fn consume_recv_credits(&mut self, qp_idx: usize, count: u32) {
        self.recv_credits[qp_idx] = self.recv_credits[qp_idx].saturating_sub(count);
    }

    /// Record receive buffer posting
    pub fn post_recv_credits(&mut self, qp_idx: usize, count: u32) {
        self.recv_credits[qp_idx] += count;
        self.posted_recv_per_qp[qp_idx] += count;
    }

    /// Record receive buffer posting for a specific message size
    pub fn post_recv_credits_for_msg_size(&mut self, qp_idx: usize, count: u32, msg_size: u32) {
        // Check if this QP is switching to a new message size
        if self.qp_recv_msg_size[qp_idx] != msg_size {
            debug!(
                qp_idx = qp_idx,
                old_qp_msg_size = self.qp_recv_msg_size[qp_idx],
                new_msg_size = msg_size,
                old_qp_buffers = self.qp_recv_buffers_for_msg_size[qp_idx],
                qp_lifetime_posted = self.posted_recv_per_qp[qp_idx],
                "QP switching to new message size - resetting QP message size tracking only"
            );

            // Update this QP's message size tracking
            self.qp_recv_msg_size[qp_idx] = msg_size;
            self.qp_recv_buffers_for_msg_size[qp_idx] = 0; // Reset count for new message size
            self.recv_credits[qp_idx] = 0; // Reset available credits for this QP
                                           // NOTE: posted_recv_per_qp[qp_idx] is NOT reset - it's lifetime accumulation

            // Update global tracking for debugging (first QP to use this message size)
            if self.current_recv_msg_size != msg_size {
                self.current_recv_msg_size = msg_size;
                self.posted_recv_for_current_size = 0;
            }
        }

        // Record the posting for this QP
        self.recv_credits[qp_idx] += count;
        self.posted_recv_per_qp[qp_idx] += count; // Lifetime accumulation (NEVER reset during message size changes)
        self.qp_recv_buffers_for_msg_size[qp_idx] += count; // Current message size count
        self.posted_recv_for_current_size += count; // Global count for current message size

        debug!(
            qp_idx = qp_idx,
            msg_size = msg_size,
            posted_count = count,
            qp_total_recv_credits = self.recv_credits[qp_idx],
            qp_lifetime_posted = self.posted_recv_per_qp[qp_idx],
            qp_buffers_for_msg_size = self.qp_recv_buffers_for_msg_size[qp_idx],
            "Updated QP receive buffer tracking with lifetime preservation"
        );
    }

    /// Get available send credits for QP
    pub fn available_send_credits(&self, qp_idx: usize) -> i32 {
        self.send_credits[qp_idx]
    }

    /// Reset flow control for new test iteration
    pub fn reset(&mut self) {
        for qp_idx in 0..self.send_credits.len() {
            self.send_credits[qp_idx] = self.max_send_credits;
            self.recv_credits[qp_idx] = 0; // Start with 0 receive buffers posted
            self.posted_recv_per_qp[qp_idx] = 0;
            self.qp_recv_msg_size[qp_idx] = 0; // Reset per-QP message size tracking
            self.qp_recv_buffers_for_msg_size[qp_idx] = 0; // Reset per-QP buffer count
        }
        self.current_recv_msg_size = 0;
        self.posted_recv_for_current_size = 0;
    }
}

/// SEND operation executor with flow control
/// Implements perftest-style SEND operations
pub struct SendOperationExecutor {
    /// Flow control manager
    flow_control: SendFlowControl,
    /// Whether immediate data is enabled
    use_immediate_data: bool,
}

impl SendOperationExecutor {
    /// Create new SEND operation executor
    pub fn new(
        qp_count: usize,
        rx_depth: u32,
        per_qp_iterations: u32,
        total_iterations: u32,
        message_sizes: Vec<u32>,
        use_immediate_data: bool,
    ) -> Self {
        Self {
            flow_control: SendFlowControl::new(
                qp_count,
                rx_depth,
                per_qp_iterations,
                total_iterations,
                message_sizes,
            ),
            use_immediate_data,
        }
    }

    /// Post initial receive buffers for server during initialization
    /// This ensures server has sufficient receive buffers ready before sync with client
    pub fn post_initial_receive_buffers<'a>(
        &mut self,
        worker: &Worker<'a>,
        worker_context: &mut WorkerContext<'a>,
        msg_size: u32,
    ) -> Result<()> {
        debug!(
            msg_size = msg_size,
            rx_depth = self.flow_control.rx_depth,
            "Posting initial receive buffers for server"
        );

        // Post exactly rx_depth receive buffers for each QP to ensure server is ready
        for qp_idx in 0..worker_context.queue_pair_count() {
            let buffers_to_post = self.flow_control.rx_depth;

            if buffers_to_post == 0 {
                continue;
            }

            // Get QP (unchecked for performance)
            let qp = unsafe { worker_context.get_queue_pair_mut_unchecked(qp_idx) };

            // Start post receive guard
            let mut guard = qp.start_post_recv();

            // Post initial receive buffers for this QP
            for recv_idx in 0..buffers_to_post {
                let recv_addr = worker.calculate_operation_addr(recv_idx, msg_size);
                let recv_addr = recv_addr + worker.increment_size as u64; // Separate receive area
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
                .map_err(|e| anyhow::anyhow!("Failed to post initial receive buffers: {}", e))?;

            // Update flow control: record posted receive buffers
            self.flow_control
                .post_recv_credits_for_msg_size(qp_idx, buffers_to_post, msg_size);

            debug!(
                qp_idx = qp_idx,
                posted_recv = buffers_to_post,
                msg_size = msg_size,
                "Posted initial receive buffers for server QP"
            );
        }

        info!(
            msg_size = msg_size,
            qp_count = worker_context.queue_pair_count(),
            rx_depth = self.flow_control.rx_depth,
            "Server initial receive buffers ready for sync"
        );

        Ok(())
    }

    /// Post SEND operations to QPs with flow control
    pub fn post_send_operations<'a>(
        &mut self,
        worker: &Worker<'a>,
        worker_context: &mut WorkerContext<'a>,
        post_list: usize,
        msg_size: u32,
    ) -> Result<()> {
        let tx_depth = worker.tx_depth;
        let thread_id_shifted = (worker.thread_id as u64) << 32;
        let qp_count = worker_context.queue_pair_count();

        debug!(
            msg_size = msg_size,
            post_list = post_list,
            qp_count = qp_count,
            "Starting post_send_operations"
        );

        // perftest approach: each QP posts operations based on available send credits
        for qp_idx in 0..qp_count {
            // Check flow control: can this QP send?
            if !self.flow_control.can_send(qp_idx) {
                debug!(
                    qp_idx = qp_idx,
                    send_credits = self.flow_control.available_send_credits(qp_idx),
                    "Skipping QP - no send credits"
                );
                continue; // Skip this QP if no send credits available
            }

            // Calculate how many operations this QP can actually send
            let available_credits = self.flow_control.available_send_credits(qp_idx) as usize;
            let actual_post_list = post_list.min(available_credits);

            if actual_post_list == 0 {
                debug!(qp_idx = qp_idx, "Skipping QP - actual_post_list is 0");
                continue; // No credits available for this QP
            }

            // perftest-style per-QP flow control check
            if !worker_context.can_qp_post_request(qp_idx, tx_depth) {
                debug!(
                    qp_idx = qp_idx,
                    inflight = worker_context.qp_send_counts[qp_idx]
                        - worker_context.qp_completion_counts[qp_idx],
                    tx_depth = tx_depth,
                    "Skipping QP - at tx_depth limit"
                );
                continue; // Skip this QP if it's at tx_depth limit
            }

            // Pre-calculate base values for this QP's operations
            let qp_operation_base = worker_context.qp_send_counts[qp_idx];

            // Get QP (unchecked for performance)
            // SAFETY: qp_idx < qp_count, which is the number of QPs we created
            let qp = unsafe { worker_context.get_queue_pair_mut_unchecked(qp_idx) };

            // Create post guard for this QP
            let mut guard = qp.start_post_send();

            // Each QP posts actual_post_list SEND operations
            for i in 0..actual_post_list {
                // Global index for wr_id tracking (includes QP information)
                let global_op_index = qp_operation_base + i as u32;
                // Encode QP index in wr_id for proper completion tracking
                // Format: [thread_id:32][qp_idx:16][op_index:16]
                let wr_id =
                    thread_id_shifted | ((qp_idx as u64) << 16) | (global_op_index & 0xFFFF) as u64;

                // Calculate local address for this operation
                let operation_index = (qp_operation_base + i as u32) % tx_depth;
                let local_addr = worker.calculate_operation_addr(operation_index, msg_size);

                // Create SEND work request (start with basic SEND, immediate data support to be added later)
                let send_handle = guard
                    .construct_wr(wr_id, WorkRequestFlags::Signaled)
                    .setup_send();

                // Setup scatter-gather entry
                unsafe {
                    send_handle.setup_sge(worker.lkey(), local_addr, msg_size);
                }
            }

            // Post all SEND operations for this QP
            guard.post()?;

            // Update flow control: consume send credits
            self.flow_control
                .consume_send_credits(qp_idx, actual_post_list as u32);

            // Update per-QP counters (perftest-style scnt tracking)
            worker_context.record_qp_requests_posted(qp_idx, actual_post_list as u32);

            debug!(
                qp_idx = qp_idx,
                posted = actual_post_list,
                remaining_credits = self.flow_control.available_send_credits(qp_idx),
                "Posted SEND operations"
            );
        }

        Ok(())
    }

    /// Post receive buffers for SEND operations with flow control
    pub fn post_receive_buffers<'a>(
        &mut self,
        worker: &Worker<'a>,
        worker_context: &mut WorkerContext<'a>,
        msg_size: u32,
    ) -> Result<()> {
        use sideway::ibverbs::queue_pair::{QueuePair, SetScatterGatherEntry};

        for qp_idx in 0..worker_context.queue_pair_count() {
            // Check if this QP needs more receive buffers for this message size
            if !self
                .flow_control
                .needs_recv_buffers_for_msg_size(qp_idx, msg_size)
            {
                continue;
            }

            // Calculate how many receive buffers to post based on actual need
            let needed_buffers =
                self.flow_control.rx_depth - self.flow_control.recv_credits[qp_idx];

            let remaining_iterations = worker_context.iterations * worker_context.round
                - worker_context.qp_completion_counts[qp_idx]
                - self.flow_control.recv_credits[qp_idx];

            // Don't post more buffers than we have remaining iterations
            let buffers_to_post = needed_buffers.min(remaining_iterations);

            // let buffers_to_post = if self.flow_control.qp_recv_msg_size[qp_idx] != msg_size {
            //     // New message size: post conservatively, but respect remaining iterations
            //     needed_buffers.min(max_buffers_needed).min(64) // Cap at 64 for transitions
            // } else {
            //     // Same message size: post exactly what we need for remaining iterations
            //     needed_buffers.min(max_buffers_needed)
            // };

            if buffers_to_post == 0 {
                debug!(
                    qp_idx = qp_idx,
                    msg_size = msg_size,
                    needed_buffers = needed_buffers,
                    recv_credits = self.flow_control.recv_credits[qp_idx],
                    remaining_iterations = remaining_iterations,
                    "Skipping buffer posting - no buffers needed"
                );
                continue;
            }

            debug!(
                qp_idx = qp_idx,
                msg_size = msg_size,
                needed_buffers = needed_buffers,
                buffers_to_post = buffers_to_post,
                total_requests = worker_context.iterations,
                complete_requests = worker_context.qp_completion_counts[qp_idx],
                recv_credits = self.flow_control.recv_credits[qp_idx],
                qp_msg_size = self.flow_control.qp_recv_msg_size[qp_idx],
                remaining_iterations = remaining_iterations,
                "Calculated iteration-aware buffer posting"
            );

            // Get QP (unchecked for performance)
            // SAFETY: qp_idx < queue_pair_count(), which is the number of QPs we created
            let qp = unsafe { worker_context.get_queue_pair_mut_unchecked(qp_idx) };

            // Start post receive guard
            let mut guard = qp.start_post_recv();

            // Post receive buffers for this QP
            for recv_idx in 0..buffers_to_post {
                // CRITICAL FIX: Use lifetime buffer accumulator to avoid address conflicts
                // posted_recv_per_qp tracks ALL buffers ever posted for this QP (never reset)
                // This ensures unique buffer addresses across message size transitions
                let recv_buffer_offset = self.flow_control.posted_recv_per_qp[qp_idx] + recv_idx;
                let recv_addr = worker.calculate_operation_addr(recv_buffer_offset, msg_size);

                // Add offset to separate receive area from send area
                let recv_addr = recv_addr + worker.increment_size as u64;

                let wr_id = (qp_idx as u64) << 32 | recv_buffer_offset as u64;

                debug!(
                    qp_idx = qp_idx,
                    recv_idx = recv_idx,
                    recv_buffer_offset = recv_buffer_offset,
                    msg_size = msg_size,
                    qp_lifetime_posted = self.flow_control.posted_recv_per_qp[qp_idx],
                    qp_buffers_for_msg_size =
                        self.flow_control.qp_recv_buffers_for_msg_size[qp_idx],
                    "Posting receive buffer with lifetime-based offset calculation"
                );

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

            // Update flow control: record posted receive buffers for this message size
            self.flow_control
                .post_recv_credits_for_msg_size(qp_idx, buffers_to_post, msg_size);

            debug!(
                qp_idx = qp_idx,
                posted_recv = buffers_to_post,
                total_recv_credits = self.flow_control.recv_credits[qp_idx],
                msg_size = msg_size,
                "Posted receive buffers"
            );
        }

        Ok(())
    }

    /// Handle completion and update flow control
    pub fn handle_send_completion(&mut self, qp_idx: usize, completion_count: u32) {
        // Restore send credits when SEND operations complete
        self.flow_control
            .restore_send_credits(qp_idx, completion_count);
    }

    /// Handle receive completion and update flow control
    pub fn handle_recv_completion(&mut self, qp_idx: usize, completion_count: u32) {
        // Consume receive credits when receive operations complete
        self.flow_control
            .consume_recv_credits(qp_idx, completion_count);
    }

    /// Reset flow control for new test iteration
    pub fn reset(&mut self) {
        self.flow_control.reset();
    }

    /// Reset flow control for new message size (clears all receive buffer state)
    pub fn reset_for_new_message_size(&mut self) {
        // Reset all flow control state as if starting fresh
        self.flow_control.reset();

        // Note: This doesn't actually drain existing receive buffers from the QP,
        // but resets our accounting. The RDMA hardware will still have the old buffers,
        // but we'll account for them as they get consumed by new operations.
        debug!("Reset flow control for new message size");
    }

    /// Get flow control reference for monitoring
    pub fn flow_control(&self) -> &SendFlowControl {
        &self.flow_control
    }
}

/// Execute SEND bandwidth test with flow control
pub fn execute_send_bandwidth_test<'a>(
    worker: &Worker<'a>,
    worker_context: &mut WorkerContext<'a>,
    msg_size: u32,
    rx_depth: u32,
    use_immediate_data: bool,
) -> Result<()> {
    // Use the exact message size - our smart flow control will handle message size changes
    // RX/TX depth is optimized to min(configured_depth, total_iterations) to avoid over-posting

    let mut send_executor = SendOperationExecutor::new(
        worker_context.queue_pair_count(),
        rx_depth,
        worker_context.iterations,
        worker_context.total_requests,
        vec![msg_size], // Single message size
        use_immediate_data,
    );

    // Only post initial receive buffers if we're running as a server
    // Client side sends data, server side receives data
    // Use the new post_initial_receive_buffers method for better control
    if worker.plan.base().server {
        debug!(
            msg_size = msg_size,
            "Posting initial receive buffers for server using dedicated method"
        );
        send_executor.post_initial_receive_buffers(worker, worker_context, msg_size)?;
    }

    info!(
        msg_size = msg_size,
        total_requests = worker_context.total_requests,
        qp_count = worker_context.queue_pair_count(),
        tx_depth = worker.tx_depth,
        "Starting SEND bandwidth test with flow control"
    );

    // Bandwidth test: post operations in batches and wait for completions
    let mut iteration = 0;
    while !worker_context.is_complete() {
        iteration += 1;

        // Only ensure receive buffers are available on server side when needed
        // Don't post every iteration - only when actually needed based on flow control
        if worker.plan.base().server && iteration % 10 == 0 {
            // Check if we actually need more buffers before posting
            let needs_buffers = (0..worker_context.queue_pair_count()).any(|qp_idx| {
                send_executor
                    .flow_control()
                    .needs_recv_buffers_for_msg_size(qp_idx, msg_size)
            });

            if needs_buffers {
                send_executor.post_receive_buffers(worker, worker_context, msg_size)?;
            }
        }

        // Only post SEND operations if we're a client (in unidirectional mode)
        // In bidirectional mode, both client and server post SEND operations
        if (!worker.plan.base().server || worker.plan.base().bidir)
            && worker_context.can_post_request(worker.tx_depth)
        {
            // Calculate batch size based on remaining requests
            let remaining_requests =
                worker_context.total_requests - worker_context.completed_requests;
            let post_list_size = worker.plan.base().post_list.min(remaining_requests) as usize;

            if post_list_size > 0 {
                debug!(
                    iteration = iteration,
                    msg_size = msg_size,
                    remaining_requests = remaining_requests,
                    post_list_size = post_list_size,
                    completed = worker_context.completed_requests,
                    total = worker_context.total_requests,
                    is_server = worker.plan.base().server,
                    is_bidir = worker.plan.base().bidir,
                    "About to post SEND operations"
                );
                send_executor.post_send_operations(
                    worker,
                    worker_context,
                    post_list_size,
                    msg_size,
                )?;
            }
        }

        // Poll for completions more frequently
        // For server in unidirectional mode, receive completions count as progress
        let track_recv_progress = worker.plan.base().server && !worker.plan.base().bidir;
        let (_send_completions, recv_completions) =
            poll_send_completions(worker_context, &mut send_executor, track_recv_progress)?;

        // If we processed receive completions, intelligently reload receive buffers (server only)
        // CRITICAL FIX: Only post buffers if we don't have enough for remaining iterations
        if recv_completions > 0 && worker.plan.base().server {
            let remaining_iterations =
                worker_context.total_requests - worker_context.completed_requests;

            // Only post if we don't have enough receive credits for remaining iterations
            let should_reload = remaining_iterations > 0
                && (0..worker_context.queue_pair_count()).any(|qp_idx| {
                    let credits = send_executor.flow_control().recv_credits[qp_idx];
                    // Need buffers if current credits are insufficient for remaining iterations
                    credits < remaining_iterations
                });

            if should_reload {
                debug!(
                    recv_completions = recv_completions,
                    remaining_iterations = remaining_iterations,
                    completed = worker_context.completed_requests,
                    total = worker_context.total_requests,
                    "Posting receive buffers based on remaining iterations"
                );
                send_executor.post_receive_buffers(worker, worker_context, msg_size)?;
            } else if remaining_iterations > 0 {
                debug!(
                    recv_completions = recv_completions,
                    remaining_iterations = remaining_iterations,
                    "Skipping buffer posting - sufficient credits for remaining iterations"
                );
            }
        }

        // Add some status logging every 10000 iterations
        // if iteration % 10000 == 0 {
        //     debug!(
        //         iteration = iteration,
        //         msg_size = msg_size,
        //         completed = worker_context.completed_requests,
        //         total = worker_context.total_requests,
        //         progress = (worker_context.completed_requests as f64 / worker_context.total_requests as f64) * 100.0,
        //         "Bandwidth test progress"
        //     );
        // }
    }

    // Wait for any remaining completions
    while worker_context.completed_requests < worker_context.total_requests {
        let track_recv_progress = worker.plan.base().server && !worker.plan.base().bidir;
        let (_send_completions, recv_completions) =
            poll_send_completions(worker_context, &mut send_executor, track_recv_progress)?;

        // If we processed receive completions, intelligently reload receive buffers (server only)
        // CRITICAL FIX: Only post buffers if we don't have enough for remaining iterations
        if recv_completions > 0 && worker.plan.base().server {
            let remaining_iterations =
                worker_context.total_requests - worker_context.completed_requests;

            // Only post if we don't have enough receive credits for remaining iterations
            let should_reload = remaining_iterations > 0
                && (0..worker_context.queue_pair_count()).any(|qp_idx| {
                    let credits = send_executor.flow_control().recv_credits[qp_idx];
                    // Need buffers if current credits are insufficient for remaining iterations
                    credits < remaining_iterations
                });

            if should_reload {
                debug!(
                    recv_completions = recv_completions,
                    remaining_iterations = remaining_iterations,
                    completed = worker_context.completed_requests,
                    total = worker_context.total_requests,
                    "Posting receive buffers in wait loop based on remaining iterations"
                );
                send_executor.post_receive_buffers(worker, worker_context, msg_size)?;
            }
        }
    }

    Ok(())
}

/// Execute SEND latency test with flow control
pub fn execute_send_latency_test<'a>(
    worker: &Worker<'a>,
    worker_context: &mut WorkerContext<'a>,
    msg_size: u32,
    rx_depth: u32,
    use_immediate_data: bool,
    histogram: &mut hdrhistogram::Histogram<u64>,
    clock: &quanta::Clock,
) -> Result<()> {
    // Use the exact message size - our smart flow control will handle message size changes
    // RX/TX depth is optimized to min(configured_depth, total_iterations) to avoid over-posting

    let mut send_executor = SendOperationExecutor::new(
        worker_context.queue_pair_count(),
        rx_depth,
        worker_context.iterations,
        worker_context.total_requests,
        vec![msg_size], // Single message size
        use_immediate_data,
    );

    // Only post initial receive buffers if we're running as a server
    // Use the new post_initial_receive_buffers method for better control
    if worker.plan.base().server {
        debug!(
            msg_size = msg_size,
            "Posting initial receive buffers for latency test using dedicated method"
        );
        send_executor.post_initial_receive_buffers(worker, worker_context, msg_size)?;
    }

    let cq = worker_context.completion_queue().clone();
    // Server needs both SEND and RECV completions, client only needs SEND completions
    let mut completions_needed = if worker.plan.base().server {
        worker_context.total_requests * 2 // SEND + RECV completions
    } else {
        worker_context.total_requests // Only SEND completions for client
    };

    info!(
        msg_size = msg_size,
        total_requests = worker_context.total_requests,
        qp_count = worker_context.queue_pair_count(),
        "Starting SEND latency test with flow control"
    );

    while !worker_context.is_complete() || completions_needed > 0 {
        // Only post SEND operations if we're a client (in unidirectional mode)
        // In bidirectional mode, both client and server post SEND operations
        if (!worker.plan.base().server || worker.plan.base().bidir)
            && worker_context.can_post_request(worker.tx_depth)
        {
            let start_time = clock.now();

            // Post single SEND operation
            send_executor.post_send_operations(worker, worker_context, 1, msg_size)?;

            // Wait for completion (SEND + RECV for server, SEND only for client)
            wait_for_send_completion(
                &cq,
                start_time,
                histogram,
                worker_context,
                &mut send_executor,
                clock,
                worker.plan.base().server,
            )?;

            // Server expects 2 completions (SEND + RECV), client expects 1 (SEND only)
            let expected_completions = if worker.plan.base().server { 2 } else { 1 };
            completions_needed = completions_needed.saturating_sub(expected_completions);
        }

        // Post more receive buffers if needed (server only)
        if worker.plan.base().server {
            send_executor.post_receive_buffers(worker, worker_context, msg_size)?;
        }
    }

    Ok(())
}

/// Wait for SEND completion and handle flow control
fn wait_for_send_completion(
    cq: &Rc<RefCell<sideway::ibverbs::completion::ExtendedCompletionQueue>>,
    start_time: quanta::Instant,
    histogram: &mut hdrhistogram::Histogram<u64>,
    worker_context: &mut WorkerContext,
    send_executor: &mut SendOperationExecutor,
    clock: &quanta::Clock,
    is_server: bool,
) -> Result<()> {
    let timeout = std::time::Duration::from_secs(30);
    let deadline = std::time::Instant::now() + timeout;

    let mut send_completed = false;
    let mut recv_completed = false;

    // Client only waits for SEND completion, server waits for both SEND and RECV
    while (!send_completed || (is_server && !recv_completed))
        && std::time::Instant::now() < deadline
    {
        // Poll for completions
        match cq.borrow_mut().start_poll() {
            Ok(mut poller) => {
                while let Some(wc) = poller.next() {
                    if wc.status() != WorkCompletionStatus::Success as u32 {
                        return Err(anyhow::anyhow!(
                            "Failed status {:?} ({}) for wr_id {}",
                            Into::<WorkCompletionStatus>::into(wc.status()),
                            wc.status(),
                            wc.wr_id()
                        ));
                    }

                    // Extract QP index from wr_id
                    let qp_idx = ((wc.wr_id() >> 16) & 0xFFFF) as usize;

                    // Determine if this is SEND or RECV completion
                    if wc.wr_id() & 0xFFFF < 32768 {
                        // SEND completion (lower wr_id range)
                        send_executor.handle_send_completion(qp_idx, 1);
                        worker_context.record_qp_requests_completed(qp_idx, 1);

                        // Measure latency for SEND completion
                        let completion_time = clock.now();
                        let latency_ns = completion_time.duration_since(start_time).into_nanos();
                        histogram.record(latency_ns)?;

                        send_completed = true;
                    } else {
                        // RECV completion (higher wr_id range)
                        send_executor.handle_recv_completion(qp_idx, 1);
                        recv_completed = true;
                    }
                }
            }
            Err(_) => {
                continue; // No completions available
            }
        }
    }

    // Check completion based on client/server mode
    if !send_completed || (is_server && !recv_completed) {
        let expected_recv = if is_server {
            "required"
        } else {
            "not required"
        };
        return Err(anyhow::anyhow!(
            "Timeout waiting for completions. Send: {}, Recv: {} ({})",
            send_completed,
            recv_completed,
            expected_recv
        ));
    }

    Ok(())
}

/// Poll for SEND completions without blocking (from both send and recv CQs)
/// Returns Ok((send_completions, recv_completions)) indicating how many completions were processed
fn poll_send_completions(
    worker_context: &mut WorkerContext,
    send_executor: &mut SendOperationExecutor,
    track_recv_for_progress: bool, // True if receive completions should count toward overall progress
) -> Result<(u32, u32)> {
    let mut send_completions = 0;
    let mut recv_completions = 0;

    // Use two-step approach to avoid borrow conflicts:
    // 1. Poll and collect completion info
    // 2. Process completions

    // Step 1: Poll send completion queue
    let send_cq = worker_context.send_completion_queue().clone();
    let mut send_completion_info = Vec::new();

    match send_cq.borrow_mut().start_poll() {
        Ok(mut poller) => {
            while let Some(wc) = poller.next() {
                if wc.status() != WorkCompletionStatus::Success as u32 {
                    // Enhanced error logging
                    let qp_idx = ((wc.wr_id() >> 16) & 0xFFFF) as usize;
                    let send_credits = send_executor.flow_control().available_send_credits(qp_idx);
                    let recv_credits = send_executor.flow_control().recv_credits[qp_idx];

                    return Err(anyhow::anyhow!(
                        "SEND Failed status {:?} ({}) for wr_id {} (QP {}, send_credits: {}, recv_credits: {})",
                        Into::<WorkCompletionStatus>::into(wc.status()),
                        wc.status(),
                        wc.wr_id(),
                        qp_idx,
                        send_credits,
                        recv_credits
                    ));
                }

                // Extract QP index from wr_id
                let qp_idx = ((wc.wr_id() >> 16) & 0xFFFF) as usize;
                send_completion_info.push(qp_idx);
            }
        }
        Err(_) => {
            // No completions available, continue
        }
    }

    // Step 2: Process send completions (CQ borrow is dropped by now)
    for qp_idx in send_completion_info {
        send_executor.handle_send_completion(qp_idx, 1);
        worker_context.record_qp_requests_completed(qp_idx, 1);
        send_completions += 1;
    }

    // Step 3: Poll receive completion queue if it exists
    let mut recv_completion_info = Vec::new();

    if let Some(recv_cq) = worker_context.recv_completion_queue() {
        let recv_cq = recv_cq.clone();
        match recv_cq.borrow_mut().start_poll() {
            Ok(mut poller) => {
                while let Some(wc) = poller.next() {
                    if wc.status() != WorkCompletionStatus::Success as u32 {
                        // Enhanced error logging
                        let qp_idx = ((wc.wr_id() >> 32) & 0xFFFF) as usize;
                        let send_credits =
                            send_executor.flow_control().available_send_credits(qp_idx);
                        let recv_credits = send_executor.flow_control().recv_credits[qp_idx];

                        return Err(anyhow::anyhow!(
                            "RECV Failed status {:?} ({}) for wr_id {} (QP {}, send_credits: {}, recv_credits: {})",
                            Into::<WorkCompletionStatus>::into(wc.status()),
                            wc.status(),
                            wc.wr_id(),
                            qp_idx,
                            send_credits,
                            recv_credits
                        ));
                    }

                    // Extract QP index from wr_id (different encoding for recv)
                    let qp_idx = ((wc.wr_id() >> 32) & 0xFFFF) as usize;
                    recv_completion_info.push(qp_idx);
                }
            }
            Err(_) => {
                // No completions available, continue
            }
        };
    }

    // Step 4: Process recv completions (CQ borrow is dropped by now)
    for qp_idx in recv_completion_info {
        send_executor.handle_recv_completion(qp_idx, 1);
        recv_completions += 1;

        // For server in unidirectional mode, receive completions count as progress
        if track_recv_for_progress {
            worker_context.record_qp_requests_completed(qp_idx, 1);
        }
    }

    // Step 5: If we processed any receive completions, immediately reload receive buffers
    if recv_completions > 0 {
        // We can't pass worker and msg_size here since we don't have access to them
        // This will be handled by the caller calling post_receive_buffers() regularly
        debug!(
            recv_completions = recv_completions,
            "Receive completions processed - need to reload receive buffers"
        );
    }

    if send_completions > 0 || recv_completions > 0 {
        debug!(
            send_completions = send_completions,
            recv_completions = recv_completions,
            total_completed = worker_context.completed_requests,
            total_requests = worker_context.total_requests,
            "Processed completions"
        );
    }

    Ok((send_completions, recv_completions))
}
