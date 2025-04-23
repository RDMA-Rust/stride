pub trait CommandContext {
    /// Get the target device name
    fn device(&self) -> Option<&str>;

    /// Set the target device name
    fn set_device(&mut self, device: String);

    /// Get the GID index
    fn gid_index(&self) -> Option<u8>;

    /// Set the GID index
    fn set_gid_index(&mut self, index: u8);

    /// Get the number of iterations
    fn iterations(&self) -> u32;

    /// Set the number of iterations
    fn set_iterations(&mut self, iters: u32);

    /// Get the message size in bytes
    fn message_size(&self) -> u32;

    /// Set the message size in bytes
    fn set_message_size(&mut self, size: u32);

    /// Get the QP timeout value
    fn qp_timeout(&self) -> u8;

    /// Set the QP timeout value
    fn set_qp_timeout(&mut self, timeout: u8);

    /// Get the TX queue depth
    fn tx_depth(&self) -> Option<u32>;

    /// Set the TX queue depth
    fn set_tx_depth(&mut self, depth: u32);

    /// Get the RX queue depth
    fn rx_depth(&self) -> Option<u32>;

    /// Set the RX queue depth
    fn set_rx_depth(&mut self, depth: u32);

    /// Whether immediate data should be used
    fn use_immediate_data(&self) -> bool;

    /// Set whether immediate data should be used
    fn set_use_immediate_data(&mut self, use_imm: bool);

    /// Whether to run the test with all message sizes
    fn all_sizes(&self) -> bool {
        false
    }

    /// Set whether to run the test with all message sizes
    fn set_all_sizes(&mut self, _all_sizes: bool) {
        // Default implementation does nothing
    }

    /// Get the step factor for message sizes when running all sizes
    fn step_factor(&self) -> f64 {
        2.0
    }

    /// Set the step factor for message sizes
    fn set_step_factor(&mut self, _factor: f64) {
        // Default implementation does nothing
    }

    /// Create a descriptive name for this operation
    fn operation_name(&self) -> String;

    /// Get whether this endpoint should act as a server
    fn server_mode(&self) -> bool {
        self.address().is_none()
    }

    /// Get the port number to listen on (server) or connect to (client)
    fn port(&self) -> Option<u16> {
        None
    }

    /// Set the port number
    fn set_port(&mut self, _port: u16) {
        // Default implementation does nothing
    }

    /// Get the target address for client connections
    fn address(&self) -> Option<String> {
        None
    }

    /// Set the target address for client connections
    fn set_address(&mut self, _address: String) {
        // Default implementation does nothing
    }

    /// Get the number of QPs to use
    fn qp_count(&self) -> Option<usize> {
        None
    }

    /// Set the number of QPs to use
    fn set_qp_count(&mut self, _count: usize) {
        // Default implementation does nothing
    }

    /// Get the CQE poll batch size
    fn cqe_poll(&self) -> u32 {
        32 // Default value
    }

    /// Set the CQE poll batch size
    fn set_cqe_poll(&mut self, _size: u32) {
        // Default implementation does nothing
    }

    fn bidirectional(&self) -> bool {
        false
    }

    /// Get the number of WQEs to post in a single batch
    fn post_list(&self) -> u32 {
        1
    }

    /// Set the number of WQEs to post in a single batch
    fn set_post_list(&mut self, _count: u32) {
        // Default implementation does nothing
    }
}
