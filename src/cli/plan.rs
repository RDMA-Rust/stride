use crate::connection::exchange::MemoryRegionInfo;
use sideway::ibverbs::device_context::Mtu;

#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub tui_enabled: bool,
    pub trace_enabled: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            tui_enabled: true,
            trace_enabled: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlanBase {
    pub mode: Mode,
    pub dev: Option<String>,
    pub gid_index: Option<u8>,
    pub server: bool,
    pub addr: String,
    pub threads: usize,
    pub msg_sizes: Vec<u32>,
    pub iters: u32,
    pub bidir: bool,
    pub tx_depth: u32,
    pub timeout: u8,
    pub post_list: u32,
    pub cqe_poll: u32,
    pub hugepages: bool,
    pub mtu: Mtu,
    pub output: OutputConfig,
}

#[derive(Debug, Clone)]
pub enum Plan {
    Send(SendPlan),
    Write(WritePlan),
    Read(ReadPlan),
}

#[derive(Debug, Clone)]
pub struct SendPlan {
    pub base: PlanBase,
    pub rx_depth: u32,
    pub imm_data: bool,
    pub flow_control: bool,
}

#[derive(Debug, Clone)]
pub struct WritePlan {
    pub base: PlanBase,
    pub imm_data: bool,
    pub remote_mr: Option<MemoryRegionInfo>,
}

#[derive(Debug, Clone)]
pub struct ReadPlan {
    pub base: PlanBase,
    pub remote_mr: Option<MemoryRegionInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Send,
    Write,
    Read,
    Atomic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Bandwidth,
    Latency,
}

impl Plan {
    pub fn base(&self) -> &PlanBase {
        match self {
            Plan::Send(p) => &p.base,
            Plan::Write(p) => &p.base,
            Plan::Read(p) => &p.base,
        }
    }

    pub fn operation(&self) -> Operation {
        match self {
            Plan::Send(_) => Operation::Send,
            Plan::Write(_) => Operation::Write,
            Plan::Read(_) => Operation::Read,
        }
    }

    pub fn poll_batch(&self) -> u32 {
        self.base().cqe_poll
    }

    pub fn is_latency(&self) -> bool {
        self.base().mode == Mode::Latency
    }

    pub fn needs_remote_addr(&self) -> bool {
        matches!(self.operation(), Operation::Write | Operation::Read)
    }

    pub fn test_name(&self) -> &'static str {
        match (self.operation(), self.base().mode) {
            (Operation::Send, Mode::Bandwidth) => "SEND bw",
            (Operation::Send, Mode::Latency) => "SEND lat",
            (Operation::Write, Mode::Bandwidth) => "WRITE bw",
            (Operation::Write, Mode::Latency) => "WRITE lat",
            (Operation::Read, Mode::Bandwidth) => "READ bw",
            (Operation::Read, Mode::Latency) => "READ lat",
            (Operation::Atomic, Mode::Bandwidth) => "ATOMIC bw",
            (Operation::Atomic, Mode::Latency) => "ATOMIC lat",
        }
    }

    pub fn rx_depth(&self) -> Option<u32> {
        match self {
            Plan::Send(p) => Some(p.rx_depth),
            _ => None, // WRITE and READ don't need rx_depth
        }
    }

    pub fn uses_immediate_data(&self) -> bool {
        match self {
            Plan::Send(p) => p.imm_data,
            Plan::Write(p) => p.imm_data,
            Plan::Read(_) => false, // READ doesn't support immediate data
        }
    }

    pub fn uses_flow_control(&self) -> bool {
        match self {
            Plan::Send(p) => p.flow_control,
            _ => false, // Only SEND supports flow control
        }
    }

    /// Get remote memory region (for one-sided operations)
    pub fn remote_mr(&self) -> Option<&MemoryRegionInfo> {
        match self {
            Plan::Send(_) => None, // SEND doesn't use remote MR
            Plan::Write(p) => p.remote_mr.as_ref(),
            Plan::Read(p) => p.remote_mr.as_ref(),
        }
    }

    /// Set remote memory region (called during connection setup)
    pub fn set_remote_mr(&mut self, remote_mr: MemoryRegionInfo) {
        match self {
            Plan::Send(_) => {} // SEND doesn't use remote MR
            Plan::Write(p) => p.remote_mr = Some(remote_mr),
            Plan::Read(p) => p.remote_mr = Some(remote_mr),
        }
    }

    /// Get the configured MTU for this plan
    pub fn mtu(&self) -> Mtu {
        self.base().mtu
    }
}

/// Execute a benchmark plan using the new worker architecture
pub fn execute(plan: Plan) -> anyhow::Result<()> {
    use crate::runners::worker::run_worker;
    run_worker(plan)
}
