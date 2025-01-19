use super::context::CommandContext;
use crate::cli::perf::{CommonArgs, SendBandwidthArgs};

/// Base parameters shared by all commands
#[derive(Debug, Clone)]
pub struct BaseParams {
    device: Option<String>,
    gid_index: Option<u32>,
    iterations: u32,
    message_size: u32,
    qp_timeout: u8,
    tx_depth: Option<u32>,
    rx_depth: Option<u32>,
    use_imm_data: bool,
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
        }
    }
}

/// Send bandwidth command parameters
#[derive(Debug, Clone)]
pub struct SendBandwidthParams {
    base: BaseParams,
}

impl SendBandwidthParams {
    pub fn new() -> Self {
        let mut base = BaseParams::default();
        base.tx_depth = Some(128);
        Self { base }
    }

    pub fn from_args(args: &SendBandwidthArgs) -> Self {
        let mut params = Self::new();
        params.set_from_common_args(&args.common);
        params.set_tx_depth(args.tx_depth);
        if let Some(rx_depth) = args.rx_depth {
            params.set_rx_depth(rx_depth);
        }
        params.set_use_immediate_data(args.imm_data);
        params
    }

    fn set_from_common_args(&mut self, args: &CommonArgs) {
        if let Some(device) = &args.device {
            self.set_device(device.clone());
        }
        if let Some(gid_index) = args.gid_index {
            self.set_gid_index(gid_index);
        }
        self.set_iterations(args.iters);
        self.set_message_size(args.msg_size);
        self.set_qp_timeout(args.qp_timeout);
    }
}

impl CommandContext for SendBandwidthParams {
    fn device(&self) -> Option<&str> {
        self.base.device.as_deref()
    }

    fn set_device(&mut self, device: String) {
        self.base.device = Some(device);
    }

    fn gid_index(&self) -> Option<u32> {
        self.base.gid_index
    }

    fn set_gid_index(&mut self, index: u32) {
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

    fn operation_name(&self) -> String {
        "SEND bandwidth test".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct SendLatencyParams {
    base: BaseParams,
}

impl SendLatencyParams {
    pub fn new() -> Self {
        Self { base: BaseParams::default() }
    }

    pub fn from_args(args: &crate::cli::perf::SendLatencyArgs) -> Self {
        let mut params = Self::new();
        params.set_from_common_args(&args.common);
        if let Some(tx_depth) = args.tx_depth {
            params.set_tx_depth(tx_depth);
        }
        if let Some(rx_depth) = args.rx_depth {
            params.set_rx_depth(rx_depth);
        }
        params.set_use_immediate_data(args.imm_data);
        params
    }

    fn set_from_common_args(&mut self, args: &crate::cli::perf::CommonArgs) {
        if let Some(device) = &args.device {
            self.set_device(device.clone());
        }
        if let Some(gid_index) = args.gid_index {
            self.set_gid_index(gid_index);
        }
        self.set_iterations(args.iters);
        self.set_message_size(args.msg_size);
        self.set_qp_timeout(args.qp_timeout);
    }
}

impl CommandContext for SendLatencyParams {
    fn device(&self) -> Option<&str> {
        self.base.device.as_deref()
    }

    fn set_device(&mut self, device: String) {
        self.base.device = Some(device);
    }

    fn gid_index(&self) -> Option<u32> {
        self.base.gid_index
    }

    fn set_gid_index(&mut self, index: u32) {
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

    fn operation_name(&self) -> String {
        "SEND latency test".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct WriteBandwidthParams {
    base: BaseParams,
}

impl WriteBandwidthParams {
    pub fn new() -> Self {
        let mut base = BaseParams::default();
        base.tx_depth = Some(128);
        Self { base }
    }

    pub fn from_args(args: &crate::cli::perf::WriteBandwidthArgs) -> Self {
        let mut params = Self::new();
        params.set_from_common_args(&args.common);
        params.set_tx_depth(args.tx_depth);
        params.set_use_immediate_data(args.imm_data);
        params
    }

    fn set_from_common_args(&mut self, args: &CommonArgs) {
        if let Some(device) = &args.device {
            self.set_device(device.clone());
        }
        if let Some(gid_index) = args.gid_index {
            self.set_gid_index(gid_index);
        }
        self.set_iterations(args.iters);
        self.set_message_size(args.msg_size);
        self.set_qp_timeout(args.qp_timeout);
    }
}

impl CommandContext for WriteBandwidthParams {
    // Similar to SendBandwidthParams implementation
    fn device(&self) -> Option<&str> {
        self.base.device.as_deref()
    }

    fn set_device(&mut self, device: String) {
        self.base.device = Some(device);
    }

    fn gid_index(&self) -> Option<u32> {
        self.base.gid_index
    }

    fn set_gid_index(&mut self, index: u32) {
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

    fn operation_name(&self) -> String {
        "WRITE bandwidth test".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct WriteLatencyParams {
    base: BaseParams,
}

impl WriteLatencyParams {
    pub fn new() -> Self {
        Self { base: BaseParams::default() }
    }

    pub fn from_args(args: &crate::cli::perf::WriteLatencyArgs) -> Self {
        let mut params = Self::new();
        params.set_from_common_args(&args.common);
        if let Some(tx_depth) = args.tx_depth {
            params.set_tx_depth(tx_depth);
        }
        params.set_use_immediate_data(args.imm_data);
        params
    }

    fn set_from_common_args(&mut self, args: &CommonArgs) {
        if let Some(device) = &args.device {
            self.set_device(device.clone());
        }
        if let Some(gid_index) = args.gid_index {
            self.set_gid_index(gid_index);
        }
        self.set_iterations(args.iters);
        self.set_message_size(args.msg_size);
        self.set_qp_timeout(args.qp_timeout);
    }
}

impl CommandContext for WriteLatencyParams {
    // Copy implementation from WriteBandwidthParams
    fn device(&self) -> Option<&str> {
        self.base.device.as_deref()
    }

    fn set_device(&mut self, device: String) {
        self.base.device = Some(device);
    }

    fn gid_index(&self) -> Option<u32> {
        self.base.gid_index
    }

    fn set_gid_index(&mut self, index: u32) {
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

    fn operation_name(&self) -> String {
        "WRITE latency test".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct ReadBandwidthParams {
    base: BaseParams,
}

impl ReadBandwidthParams {
    pub fn new() -> Self {
        Self { base: BaseParams::default() }
    }

    pub fn from_args(args: &crate::cli::perf::ReadBandwidthArgs) -> Self {
        let mut params = Self::new();
        params.set_from_common_args(&args.common);
        if let Some(tx_depth) = args.tx_depth {
            params.set_tx_depth(tx_depth);
        }
        params
    }

    fn set_from_common_args(&mut self, args: &CommonArgs) {
        if let Some(device) = &args.device {
            self.set_device(device.clone());
        }
        if let Some(gid_index) = args.gid_index {
            self.set_gid_index(gid_index);
        }
        self.set_iterations(args.iters);
        self.set_message_size(args.msg_size);
        self.set_qp_timeout(args.qp_timeout);
    }
}

impl CommandContext for ReadBandwidthParams {
    // Similar implementation
    fn device(&self) -> Option<&str> {
        self.base.device.as_deref()
    }

    fn set_device(&mut self, device: String) {
        self.base.device = Some(device);
    }

    fn gid_index(&self) -> Option<u32> {
        self.base.gid_index
    }

    fn set_gid_index(&mut self, index: u32) {
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

    fn operation_name(&self) -> String {
        "READ bandwidth test".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct ReadLatencyParams {
    base: BaseParams,
}

impl ReadLatencyParams {
    pub fn new() -> Self {
        Self { base: BaseParams::default() }
    }

    pub fn from_args(args: &crate::cli::perf::ReadLatencyArgs) -> Self {
        let mut params = Self::new();
        params.set_from_common_args(&args.common);
        if let Some(tx_depth) = args.tx_depth {
            params.set_tx_depth(tx_depth);
        }
        params
    }

    fn set_from_common_args(&mut self, args: &CommonArgs) {
        if let Some(device) = &args.device {
            self.set_device(device.clone());
        }
        if let Some(gid_index) = args.gid_index {
            self.set_gid_index(gid_index);
        }
        self.set_iterations(args.iters);
        self.set_message_size(args.msg_size);
        self.set_qp_timeout(args.qp_timeout);
    }
}

impl CommandContext for ReadLatencyParams {
    // Similar implementation
    fn device(&self) -> Option<&str> {
        self.base.device.as_deref()
    }

    fn set_device(&mut self, device: String) {
        self.base.device = Some(device);
    }

    fn gid_index(&self) -> Option<u32> {
        self.base.gid_index
    }

    fn set_gid_index(&mut self, index: u32) {
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

    fn operation_name(&self) -> String {
        "READ latency test".to_string()
    }
}
