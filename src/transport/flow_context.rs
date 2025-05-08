use anyhow::Result;
use sideway::ibverbs::protection_domain::ProtectionDomain;

use super::flow_control::{FlowControl, FlowControlReceiver};

/// A context for managing flow control, separating it from parameters
/// to avoid lifetime issues
pub struct FlowControlContext<'a> {
    /// Sender-side flow control (for client or bidirectional)
    pub sender: Option<FlowControl<'a>>,
    /// Receiver-side flow control (for server or bidirectional)
    pub receiver: Option<FlowControlReceiver<'a>>,
}

impl<'a> FlowControlContext<'a> {
    /// Create a new flow control context
    pub fn new(rx_depth: u32) -> Result<Self> {
        // For client: initialize sender
        let sender_credit = rx_depth / 3; // Similar to perftest's credit_cnt
        let sender = FlowControl::new(sender_credit)?;

        // For server: initialize receiver
        let receiver = FlowControlReceiver::new(rx_depth)?;

        Ok(Self {
            sender: Some(sender),
            receiver: Some(receiver),
        })
    }

    /// Register memory regions for flow control
    pub fn register_mr(&mut self, pd: &'a ProtectionDomain<'a>) -> Result<()> {
        if let Some(sender) = &mut self.sender {
            sender.register_mr(pd)?;
        }

        if let Some(receiver) = &mut self.receiver {
            receiver.register_mr(pd)?;
        }

        Ok(())
    }

    /// Get sender flow control if available
    pub fn get_sender(&self) -> Option<&FlowControl<'a>> {
        self.sender.as_ref()
    }

    /// Get receiver flow control if available
    pub fn get_receiver(&mut self) -> Option<&mut FlowControlReceiver<'a>> {
        self.receiver.as_mut()
    }

    /// Set remote memory information for sender
    pub fn set_sender_remote_info(&mut self, addr: u64, rkey: u32) {
        if let Some(sender) = &mut self.sender {
            sender.set_remote_info(addr, rkey);
        }
    }

    /// Get memory information for receiver flow control
    pub fn get_receiver_memory_info(&self) -> Option<(u32, u64)> {
        self.receiver.as_ref().and_then(|r| r.get_memory_info())
    }
}
