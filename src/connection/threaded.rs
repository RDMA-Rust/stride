use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::connection::exchange::{DestinationInfo, MemoryRegionInfo, TestResults};
use crate::connection::{ConnectionError, ConnectionParams, ConnectionResult, ConnectionType};

/// Message types for communication between main thread and connection thread
#[derive(Debug)]
pub enum ConnectionCommand {
    /// Initialize the connection manager
    Init,
    /// Listen for incoming connections
    Listen(std::net::SocketAddr),
    /// Connect to a remote peer
    Connect(std::net::SocketAddr),
    /// Accept an incoming connection
    Accept,
    /// Exchange QP information
    ExchangeQpInfo(DestinationInfo),
    /// Exchange memory region information
    ExchangeMemoryRegions(MemoryRegionInfo),
    /// Send test results
    SendResults(TestResults),
    /// Receive test results
    ReceiveResults,
    /// Close the connection
    Close,
    /// Shutdown the connection thread
    Shutdown,
}

/// Response types from connection thread to main thread
#[derive(Debug)]
pub enum ConnectionResponse {
    /// Operation completed successfully
    Success,
    /// QP info exchange completed
    QpInfoExchanged(DestinationInfo),
    /// Memory region exchange completed
    MemoryRegionExchanged(MemoryRegionInfo),
    /// Test results received
    ResultsReceived(TestResults),
    /// Error occurred
    Error(ConnectionError),
}

/// Connection data that gets passed to the main thread after successful connection setup
#[derive(Debug, Clone)]
pub struct ConnectionData {
    pub local_addr: std::net::SocketAddr,
    pub peer_addr: std::net::SocketAddr,
    pub connection_id: u64,
}

/// Threaded connection manager that runs the actual connection logic in a separate thread
pub struct ThreadedConnectionManager {
    command_tx: mpsc::Sender<ConnectionCommand>,
    response_rx: mpsc::Receiver<ConnectionResponse>,
    connection_thread: Option<thread::JoinHandle<()>>,
    connection_data: Option<ConnectionData>,
    next_connection_id: u64,
}

impl ThreadedConnectionManager {
    /// Create a new threaded connection manager with the specified underlying connection type
    pub fn new(conn_type: ConnectionType, params: ConnectionParams) -> ConnectionResult<Self> {
        let (command_tx, command_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();

        let conn_type_for_thread = conn_type;
        let connection_thread = thread::Builder::new()
            .name("connection-manager".to_string())
            .spawn(move || {
                Self::connection_thread_main(conn_type_for_thread, params, command_rx, response_tx);
            })
            .map_err(|e| ConnectionError::IoError(e))?;

        Ok(Self {
            command_tx,
            response_rx,
            connection_thread: Some(connection_thread),
            connection_data: None,
            next_connection_id: 0,
        })
    }

    /// Main function for the connection thread
    fn connection_thread_main(
        conn_type: ConnectionType,
        params: ConnectionParams,
        command_rx: mpsc::Receiver<ConnectionCommand>,
        response_tx: mpsc::Sender<ConnectionResponse>,
    ) {
        // Create the underlying connection manager
        let mut connection_manager =
            match crate::connection::ConnectionFactory::create(conn_type, params) {
                Ok(cm) => cm,
                Err(e) => {
                    let _ = response_tx.send(ConnectionResponse::Error(e));
                    return;
                }
            };

        // Process commands from the main thread
        while let Ok(command) = command_rx.recv() {
            let response = match command {
                ConnectionCommand::Init => match connection_manager.init() {
                    Ok(()) => ConnectionResponse::Success,
                    Err(e) => ConnectionResponse::Error(e),
                },
                ConnectionCommand::Listen(addr) => match connection_manager.listen(addr) {
                    Ok(()) => ConnectionResponse::Success,
                    Err(e) => ConnectionResponse::Error(e),
                },
                ConnectionCommand::Connect(addr) => match connection_manager.connect(addr) {
                    Ok(()) => ConnectionResponse::Success,
                    Err(e) => ConnectionResponse::Error(e),
                },
                ConnectionCommand::Accept => match connection_manager.accept() {
                    Ok(()) => ConnectionResponse::Success,
                    Err(e) => ConnectionResponse::Error(e),
                },
                ConnectionCommand::ExchangeQpInfo(local_data) => {
                    match connection_manager.exchange_qp_info(local_data) {
                        Ok(remote_data) => ConnectionResponse::QpInfoExchanged(remote_data),
                        Err(e) => ConnectionResponse::Error(e),
                    }
                }
                ConnectionCommand::ExchangeMemoryRegions(local_mr) => {
                    match connection_manager.exchange_memory_regions(local_mr) {
                        Ok(remote_mr) => ConnectionResponse::MemoryRegionExchanged(remote_mr),
                        Err(e) => ConnectionResponse::Error(e),
                    }
                }
                ConnectionCommand::SendResults(results) => {
                    match connection_manager.send_results(&results) {
                        Ok(()) => ConnectionResponse::Success,
                        Err(e) => ConnectionResponse::Error(e),
                    }
                }
                ConnectionCommand::ReceiveResults => match connection_manager.receive_results() {
                    Ok(results) => ConnectionResponse::ResultsReceived(results),
                    Err(e) => ConnectionResponse::Error(e),
                },
                ConnectionCommand::Close => match connection_manager.close() {
                    Ok(()) => ConnectionResponse::Success,
                    Err(e) => ConnectionResponse::Error(e),
                },
                ConnectionCommand::Shutdown => {
                    let _ = connection_manager.close();
                    break;
                }
            };

            if response_tx.send(response).is_err() {
                // Main thread has disconnected, exit
                break;
            }
        }
    }

    /// Send a command to the connection thread and wait for response
    fn send_command_and_wait(
        &self,
        command: ConnectionCommand,
    ) -> ConnectionResult<ConnectionResponse> {
        self.command_tx.send(command).map_err(|_| {
            ConnectionError::IoError(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Connection thread has died",
            ))
        })?;

        self.response_rx
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| ConnectionError::Timeout("Connection operation timed out".to_string()))
    }

    /// Initialize the connection and store connection data
    pub fn init(&mut self) -> ConnectionResult<()> {
        match self.send_command_and_wait(ConnectionCommand::Init)? {
            ConnectionResponse::Success => Ok(()),
            ConnectionResponse::Error(e) => Err(e),
            _ => Err(ConnectionError::InvalidConfiguration(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Listen for connections and store connection data when accept() is called
    pub fn listen(&mut self, addr: std::net::SocketAddr) -> ConnectionResult<()> {
        match self.send_command_and_wait(ConnectionCommand::Listen(addr))? {
            ConnectionResponse::Success => {
                self.connection_data = Some(ConnectionData {
                    local_addr: addr,
                    peer_addr: addr, // Will be updated in accept()
                    connection_id: self.next_connection_id,
                });
                self.next_connection_id += 1;
                Ok(())
            }
            ConnectionResponse::Error(e) => Err(e),
            _ => Err(ConnectionError::InvalidConfiguration(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Connect to a remote peer and store connection data
    pub fn connect(&mut self, addr: std::net::SocketAddr) -> ConnectionResult<()> {
        match self.send_command_and_wait(ConnectionCommand::Connect(addr))? {
            ConnectionResponse::Success => {
                self.connection_data = Some(ConnectionData {
                    local_addr: addr, // This will be the actual local addr from the underlying connection
                    peer_addr: addr,
                    connection_id: self.next_connection_id,
                });
                self.next_connection_id += 1;
                Ok(())
            }
            ConnectionResponse::Error(e) => Err(e),
            _ => Err(ConnectionError::InvalidConfiguration(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Accept an incoming connection
    pub fn accept(&mut self) -> ConnectionResult<()> {
        match self.send_command_and_wait(ConnectionCommand::Accept)? {
            ConnectionResponse::Success => Ok(()),
            ConnectionResponse::Error(e) => Err(e),
            _ => Err(ConnectionError::InvalidConfiguration(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Exchange QP information with the remote peer
    pub fn exchange_qp_info(
        &self,
        local_data: DestinationInfo,
    ) -> ConnectionResult<DestinationInfo> {
        match self.send_command_and_wait(ConnectionCommand::ExchangeQpInfo(local_data))? {
            ConnectionResponse::QpInfoExchanged(remote_data) => Ok(remote_data),
            ConnectionResponse::Error(e) => Err(e),
            _ => Err(ConnectionError::InvalidConfiguration(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Exchange memory region information with the remote peer
    pub fn exchange_memory_regions(
        &self,
        local_mr: MemoryRegionInfo,
    ) -> ConnectionResult<MemoryRegionInfo> {
        match self.send_command_and_wait(ConnectionCommand::ExchangeMemoryRegions(local_mr))? {
            ConnectionResponse::MemoryRegionExchanged(remote_mr) => Ok(remote_mr),
            ConnectionResponse::Error(e) => Err(e),
            _ => Err(ConnectionError::InvalidConfiguration(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Send test results to the remote peer
    pub fn send_results(&self, results: &TestResults) -> ConnectionResult<()> {
        match self.send_command_and_wait(ConnectionCommand::SendResults(results.clone()))? {
            ConnectionResponse::Success => Ok(()),
            ConnectionResponse::Error(e) => Err(e),
            _ => Err(ConnectionError::InvalidConfiguration(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Receive test results from the remote peer
    pub fn receive_results(&self) -> ConnectionResult<TestResults> {
        match self.send_command_and_wait(ConnectionCommand::ReceiveResults)? {
            ConnectionResponse::ResultsReceived(results) => Ok(results),
            ConnectionResponse::Error(e) => Err(e),
            _ => Err(ConnectionError::InvalidConfiguration(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Get the connection data (available after successful connection setup)
    pub fn connection_data(&self) -> Option<&ConnectionData> {
        self.connection_data.as_ref()
    }

    /// Close the connection
    pub fn close(&mut self) -> ConnectionResult<()> {
        match self.send_command_and_wait(ConnectionCommand::Close)? {
            ConnectionResponse::Success => {
                self.connection_data = None;
                Ok(())
            }
            ConnectionResponse::Error(e) => Err(e),
            _ => Err(ConnectionError::InvalidConfiguration(
                "Unexpected response".to_string(),
            )),
        }
    }

    /// Shutdown the connection thread
    pub fn shutdown(&mut self) -> ConnectionResult<()> {
        if let Some(handle) = self.connection_thread.take() {
            // Send shutdown command
            let _ = self.command_tx.send(ConnectionCommand::Shutdown);

            // Wait for thread to finish
            handle.join().map_err(|_| {
                ConnectionError::IoError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Failed to join connection thread",
                ))
            })?;
        }
        Ok(())
    }
}

impl Drop for ThreadedConnectionManager {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// Factory for creating threaded connection managers
pub struct ThreadedConnectionFactory;

impl ThreadedConnectionFactory {
    /// Create a new threaded connection manager
    pub fn create(
        conn_type: ConnectionType,
        params: ConnectionParams,
    ) -> ConnectionResult<ThreadedConnectionManager> {
        ThreadedConnectionManager::new(conn_type, params)
    }
}
