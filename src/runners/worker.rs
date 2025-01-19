// use sideway::ibverbs::completion::GenericCompletionQueue;
// use sideway::ibverbs::protection_domain::ProtectionDomain;
// use sideway::ibverbs::queue_pair::GenericQueuePair;
// use std::collections::HashMap;

// // Worker that handles multiple QPs
// pub struct Worker {
//     id: WorkerId,
//     context: Arc<DeviceContext>,
//     pd: Arc<ProtectionDomain>,
//     cq: Arc<GenericCompletionQueue>,
//     qps: HashMap<ConnectionId, Arc<QueuePair>>,
//     send_buffers: Vec<Buffer>,
//     recv_buffers: Vec<Buffer>,
//     core_id: Option<CoreId>,
// }

// // Worker implementation
// impl Worker {
//     pub async fn run(&mut self) -> Result<()> {
//         // Pin to core if specified
//         if let Some(core_id) = self.core_id {
//             pin_thread_to_core(core_id)?;
//         }

//         loop {
//             // Poll completion queue
//             let completions = self.cq.poll_completions()?;
//             for wc in completions {
//                 self.handle_completion(wc).await?;
//             }

//             // Process send queue for each QP
//             for (conn_id, qp) in &self.qps {
//                 self.process_sends(conn_id, qp).await?;
//             }
//         }
//     }

//     pub fn add_connection(&mut self, conn_id: ConnectionId, qp_info: QpInfo) -> Result<()> {
//         // Create new QP and add to map
//         let qp = QueuePair::new(
//             self.pd.clone(),
//             self.cq.clone(),
//             self.cq.clone(),
//             QpInitAttrs::default(),
//         )?;

//         // Initialize QP with remote info
//         qp.init_with_remote(qp_info)?;

//         self.qps.insert(conn_id, Arc::new(qp));
//         Ok(())
//     }
// }
