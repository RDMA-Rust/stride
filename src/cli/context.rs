pub trait CommandContext {
    /// Get the target device name
    fn device(&self) -> Option<&str>;

    /// Set the target device name
    fn set_device(&mut self, device: String);

    /// Get the GID index
    fn gid_index(&self) -> Option<u32>;

    /// Set the GID index
    fn set_gid_index(&mut self, index: u32);

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

    /// Create a descriptive name for this operation
    fn operation_name(&self) -> String;
}
