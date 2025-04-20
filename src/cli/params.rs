use super::context::CommandContext;
use crate::cli::perf::SendBandwidthArgs;

/// Base parameters shared by all commands
#[derive(Debug, Clone)]
pub struct BaseParams {
    device: Option<String>,
    gid_index: Option<u8>,
    iterations: u32,
    message_size: u32,
    qp_timeout: u8,
    tx_depth: Option<u32>,
    rx_depth: Option<u32>,
    use_imm_data: bool,
    address: Option<String>,
    port: u16,
    qp_count: usize,
    bidirectional: bool,
    all_sizes: bool,
    step_factor: f64,
}

impl Default for BaseParams {
    fn default() -> Self {
        Self {
            device: None,
            gid_index: None,
            iterations: 1000,
            message_size: 65536,
            qp_timeout: 14,
            tx_depth: None,
            rx_depth: None,
            use_imm_data: false,
            address: None,
            port: 18515,
            qp_count: 1,
            bidirectional: false,
            all_sizes: false,
            step_factor: 2.0,
        }
    }
}

macro_rules! impl_command_context_for_base {
    ($struct_name:ty) => {
        fn device(&self) -> Option<&str> {
            self.base.device.as_deref()
        }

        fn set_device(&mut self, device: String) {
            self.base.device = Some(device);
        }

        fn gid_index(&self) -> Option<u8> {
            self.base.gid_index
        }

        fn set_gid_index(&mut self, index: u8) {
            self.base.gid_index = Some(index);
        }

        fn iterations(&self) -> u32 {
            self.base.iterations
        }

        fn set_iterations(&mut self, iters: u32) {
            self.base.iterations = iters;
        }

        fn message_size(&self) -> u32 {
            self.base.message_size
        }

        fn set_message_size(&mut self, size: u32) {
            self.base.message_size = size;
        }

        fn qp_timeout(&self) -> u8 {
            self.base.qp_timeout
        }

        fn set_qp_timeout(&mut self, timeout: u8) {
            self.base.qp_timeout = timeout;
        }

        fn tx_depth(&self) -> Option<u32> {
            self.base.tx_depth
        }

        fn set_tx_depth(&mut self, depth: u32) {
            self.base.tx_depth = Some(depth);
        }

        fn rx_depth(&self) -> Option<u32> {
            self.base.rx_depth
        }

        fn set_rx_depth(&mut self, depth: u32) {
            self.base.rx_depth = Some(depth);
        }

        fn use_immediate_data(&self) -> bool {
            self.base.use_imm_data
        }

        fn set_use_immediate_data(&mut self, use_imm: bool) {
            self.base.use_imm_data = use_imm;
        }

        fn all_sizes(&self) -> bool {
            self.base.all_sizes
        }

        fn set_all_sizes(&mut self, all_sizes: bool) {
            self.base.all_sizes = all_sizes;
        }

        fn step_factor(&self) -> f64 {
            self.base.step_factor
        }

        fn set_step_factor(&mut self, factor: f64) {
            self.base.step_factor = factor;
        }

        fn port(&self) -> Option<u16> {
            Some(self.base.port)
        }

        fn set_port(&mut self, port: u16) {
            self.base.port = port;
        }

        fn address(&self) -> Option<String> {
            self.base.address.clone()
        }

        fn set_address(&mut self, address: String) {
            self.base.address = Some(address);
        }

        fn qp_count(&self) -> Option<usize> {
            Some(self.base.qp_count)
        }

        fn set_qp_count(&mut self, count: usize) {
            self.base.qp_count = count;
        }

        fn bidirectional(&self) -> bool {
            self.base.bidirectional
        }
    };
}

/// Send bandwidth command parameters
#[derive(Debug, Clone)]
pub struct SendBandwidthParams {
    base: BaseParams,
}

impl Default for SendBandwidthParams {
    fn default() -> Self {
        Self::new()
    }
}

impl SendBandwidthParams {
    pub fn new() -> Self {
        let mut base = BaseParams::default();
        base.tx_depth = Some(128);
        Self { base }
    }

    pub fn from_args(args: &SendBandwidthArgs) -> Self {
        let mut base = BaseParams::default();

        base.device = args.common.device.clone();
        base.gid_index = args.common.gid_index;
        base.iterations = args.common.iters;
        base.message_size = args.common.msg_size;
        base.qp_timeout = args.common.qp_timeout;
        // Use server_address for client/server determination
        base.address = args.common.server_address.clone();
        base.port = args.common.port;
        base.qp_count = args.common.qp_count as usize;
        base.bidirectional = args.common.bidirectional;
        base.all_sizes = args.common.all_sizes;
        base.step_factor = args.common.step_factor;

        base.tx_depth = Some(args.tx_depth);
        base.rx_depth = args.rx_depth;
        base.use_imm_data = args.imm_data;

        Self { base }
    }
}

impl CommandContext for SendBandwidthParams {
    impl_command_context_for_base!(SendBandwidthParams);

    fn operation_name(&self) -> String {
        "SEND bandwidth test".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct SendLatencyParams {
    base: BaseParams,
}

impl Default for SendLatencyParams {
    fn default() -> Self {
        Self::new()
    }
}

impl SendLatencyParams {
    pub fn new() -> Self {
        Self {
            base: BaseParams::default(),
        }
    }

    pub fn from_args(args: &crate::cli::perf::SendLatencyArgs) -> Self {
        let mut base = BaseParams::default();

        base.device = args.common.device.clone();
        base.gid_index = args.common.gid_index;
        base.iterations = args.common.iters;
        base.message_size = args.common.msg_size;
        base.qp_timeout = args.common.qp_timeout;
        // Use server_address for client/server determination
        base.address = args.common.server_address.clone();
        base.port = args.common.port;
        base.qp_count = args.common.qp_count as usize;
        base.bidirectional = args.common.bidirectional;
        base.all_sizes = args.common.all_sizes;
        base.step_factor = args.common.step_factor;

        base.tx_depth = args.tx_depth;
        base.rx_depth = args.rx_depth;
        base.use_imm_data = args.imm_data;

        Self { base }
    }
}

impl CommandContext for SendLatencyParams {
    impl_command_context_for_base!(SendLatencyParams);

    fn operation_name(&self) -> String {
        "SEND latency test".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct WriteBandwidthParams {
    base: BaseParams,
}

impl Default for WriteBandwidthParams {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteBandwidthParams {
    pub fn new() -> Self {
        let mut base = BaseParams::default();
        base.tx_depth = Some(128);
        Self { base }
    }

    pub fn from_args(args: &crate::cli::perf::WriteBandwidthArgs) -> Self {
        let mut base = BaseParams::default();

        base.device = args.common.device.clone();
        base.gid_index = args.common.gid_index;
        base.iterations = args.common.iters;
        base.message_size = args.common.msg_size;
        base.qp_timeout = args.common.qp_timeout;
        // Use server_address for client/server determination
        base.address = args.common.server_address.clone();
        base.port = args.common.port;
        base.qp_count = args.common.qp_count as usize;
        base.bidirectional = args.common.bidirectional;
        base.all_sizes = args.common.all_sizes;
        base.step_factor = args.common.step_factor;

        base.tx_depth = Some(args.tx_depth);
        base.rx_depth = None;
        base.use_imm_data = args.imm_data;

        Self { base }
    }
}

impl CommandContext for WriteBandwidthParams {
    impl_command_context_for_base!(WriteBandwidthParams);

    fn operation_name(&self) -> String {
        "WRITE bandwidth test".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct WriteLatencyParams {
    base: BaseParams,
}

impl Default for WriteLatencyParams {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteLatencyParams {
    pub fn new() -> Self {
        Self {
            base: BaseParams::default(),
        }
    }

    pub fn from_args(args: &crate::cli::perf::WriteLatencyArgs) -> Self {
        let mut base = BaseParams::default();

        base.device = args.common.device.clone();
        base.gid_index = args.common.gid_index;
        base.iterations = args.common.iters;
        base.message_size = args.common.msg_size;
        base.qp_timeout = args.common.qp_timeout;
        // Use server_address for client/server determination
        base.address = args.common.server_address.clone();
        base.port = args.common.port;
        base.qp_count = args.common.qp_count as usize;
        base.bidirectional = args.common.bidirectional;
        base.all_sizes = args.common.all_sizes;
        base.step_factor = args.common.step_factor;

        base.tx_depth = args.tx_depth;
        base.rx_depth = None;
        base.use_imm_data = args.imm_data;

        Self { base }
    }
}

impl CommandContext for WriteLatencyParams {
    impl_command_context_for_base!(WriteLatencyParams);

    fn operation_name(&self) -> String {
        "WRITE latency test".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct ReadBandwidthParams {
    base: BaseParams,
}

impl Default for ReadBandwidthParams {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadBandwidthParams {
    pub fn new() -> Self {
        Self {
            base: BaseParams::default(),
        }
    }

    pub fn from_args(args: &crate::cli::perf::ReadBandwidthArgs) -> Self {
        let mut base = BaseParams::default();

        base.device = args.common.device.clone();
        base.gid_index = args.common.gid_index;
        base.iterations = args.common.iters;
        base.message_size = args.common.msg_size;
        base.qp_timeout = args.common.qp_timeout;
        // Use server_address for client/server determination
        base.address = args.common.server_address.clone();
        base.port = args.common.port;
        base.qp_count = args.common.qp_count as usize;
        base.bidirectional = args.common.bidirectional;
        base.all_sizes = args.common.all_sizes;
        base.step_factor = args.common.step_factor;

        base.tx_depth = args.tx_depth;
        base.rx_depth = None;
        base.use_imm_data = false;

        Self { base }
    }
}

impl CommandContext for ReadBandwidthParams {
    impl_command_context_for_base!(ReadBandwidthParams);

    fn operation_name(&self) -> String {
        "READ bandwidth test".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct ReadLatencyParams {
    base: BaseParams,
}

impl Default for ReadLatencyParams {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadLatencyParams {
    pub fn new() -> Self {
        Self {
            base: BaseParams::default(),
        }
    }

    pub fn from_args(args: &crate::cli::perf::ReadLatencyArgs) -> Self {
        let mut base = BaseParams::default();

        base.device = args.common.device.clone();
        base.gid_index = args.common.gid_index;
        base.iterations = args.common.iters;
        base.message_size = args.common.msg_size;
        base.qp_timeout = args.common.qp_timeout;
        // Use server_address for client/server determination
        base.address = args.common.server_address.clone();
        base.port = args.common.port;
        base.qp_count = args.common.qp_count as usize;
        base.bidirectional = args.common.bidirectional;
        base.all_sizes = args.common.all_sizes;
        base.step_factor = args.common.step_factor;

        base.tx_depth = args.tx_depth;
        base.rx_depth = None;
        base.use_imm_data = false;

        Self { base }
    }
}

impl CommandContext for ReadLatencyParams {
    impl_command_context_for_base!(ReadLatencyParams);

    fn operation_name(&self) -> String {
        "READ latency test".to_string()
    }
}
