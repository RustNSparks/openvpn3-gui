// src/openvpn.rs (Cleaned up version)
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use tokio::process::Command as TokioCommand;
use tokio::time::{sleep, Duration};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected { ip: String, duration: String },
    Disconnecting,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_path: String,
    pub config_name: String,
    pub status: String,
    pub created: String,
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub packets_in: u64,
    pub packets_out: u64,
    pub connected_since: String,
    pub virtual_ip: String,
    pub remote_ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigProfile {
    pub path: String,
    pub name: String,
    pub import_time: String,
    pub owner: String,
    pub valid: bool,
}

#[derive(Debug)]
pub enum VpnMessage {
    StatusUpdate(ConnectionStatus),
    LogMessage(String),
    Error(String),
    SessionsList(Vec<SessionInfo>),
    SessionStats(SessionStats),
    ConfigsList(Vec<ConfigProfile>),
    ConfigImported(String),
}

#[derive(Debug)]
pub enum VpnCommand {
    Connect(String),
    Disconnect,
    ListSessions,
    GetSessionStats,
    ListConfigs,
    ImportConfig(String, String), // path, name
    RemoveConfig(String),
    StartLogging,
    StopLogging,
    GetStatusUpdate,
    Shutdown,
}

pub struct OpenVPN3Manager {
    status: Arc<Mutex<ConnectionStatus>>,
    current_session: Arc<Mutex<Option<String>>>,
    stats: Arc<Mutex<Option<SessionStats>>>,
    sessions: Arc<Mutex<Vec<SessionInfo>>>,
    command_tx: mpsc::Sender<VpnCommand>,
    message_tx: mpsc::Sender<VpnMessage>,
    configs: Arc<Mutex<Vec<ConfigProfile>>>,
    logging_active: Arc<Mutex<bool>>,
}

impl Clone for OpenVPN3Manager {
    fn clone(&self) -> Self {
        Self {
            status: Arc::clone(&self.status),
            current_session: Arc::clone(&self.current_session),
            stats: Arc::new(Mutex::new(None)),
            sessions: Arc::new(Mutex::new(vec![])),
            command_tx: self.command_tx.clone(),
            message_tx: self.message_tx.clone(),
            configs: Arc::new(Mutex::new(vec![])),
            logging_active: Arc::clone(&self.logging_active),
        }
    }
}

impl OpenVPN3Manager {
    pub fn new(message_tx: mpsc::Sender<VpnMessage>) -> (Self, mpsc::Receiver<VpnCommand>) {
        let (command_tx, command_rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(ConnectionStatus::Disconnected));
        let current_session = Arc::new(Mutex::new(None));
        let logging_active = Arc::new(Mutex::new(false));

        let manager = Self {
            status: Arc::clone(&status),
            current_session: Arc::clone(&current_session),
            stats: Arc::new(Mutex::new(None)),
            sessions: Arc::new(Mutex::new(vec![])),
            command_tx,
            message_tx: message_tx.clone(),
            configs: Arc::new(Mutex::new(vec![])),
            logging_active: Arc::clone(&logging_active),
        };

        (manager, command_rx)
    }

    pub fn connect(&self, config_path: String) -> Result<()> {
        self.command_tx.send(VpnCommand::Connect(config_path))?;
        Ok(())
    }

    pub fn disconnect(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::Disconnect)?;
        Ok(())
    }

    pub fn list_sessions(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::ListSessions)?;
        Ok(())
    }

    pub fn get_session_stats(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::GetSessionStats)?;
        Ok(())
    }

    pub fn list_configs(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::ListConfigs)?;
        Ok(())
    }

    pub fn import_config(&self, path: String, name: String) -> Result<()> {
        self.command_tx.send(VpnCommand::ImportConfig(path, name))?;
        Ok(())
    }

    pub fn remove_config(&self, config_path: String) -> Result<()> {
        self.command_tx.send(VpnCommand::RemoveConfig(config_path))?;
        Ok(())
    }

    pub fn start_logging(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::StartLogging)?;
        Ok(())
    }

    pub fn stop_logging(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::StopLogging)?;
        Ok(())
    }

    pub fn get_status_update(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::GetStatusUpdate)?;
        Ok(())
    }

    pub fn shutdown(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::Shutdown)?;
        Ok(())
    }

    pub fn get_status(&self) -> ConnectionStatus {
        self.status.lock().unwrap().clone()
    }

    pub async fn run_manager_loop(&self, mut command_rx: mpsc::Receiver<VpnCommand>) {
        let mut monitoring = false;
        let mut log_monitoring = false;

        // Initialize by loading existing configs and sessions
        let _ = self.handle_list_configs().await;
        let _ = self.handle_list_sessions().await;

        loop {
            // Handle commands
            if let Ok(command) = command_rx.try_recv() {
                match command {
                    VpnCommand::Connect(config_path) => {
                        if let Err(e) = self.handle_connect(&config_path).await {
                            self.set_status(ConnectionStatus::Error(e.to_string()));
                            self.send_message(VpnMessage::Error(e.to_string()));
                        } else {
                            monitoring = true;
                        }
                    }
                    VpnCommand::Disconnect => {
                        if let Err(e) = self.handle_disconnect().await {
                            self.send_message(VpnMessage::Error(e.to_string()));
                        }
                        monitoring = false;
                    }
                    VpnCommand::GetStatusUpdate => {
                        if let Err(e) = self.update_status().await {
                            self.send_message(VpnMessage::Error(e.to_string()));
                        }
                    }
                    VpnCommand::ListSessions => {
                        if let Err(e) = self.handle_list_sessions().await {
                            self.send_message(VpnMessage::Error(e.to_string()));
                        }
                    }
                    VpnCommand::GetSessionStats => {
                        if let Err(e) = self.handle_get_session_stats().await {
                            self.send_message(VpnMessage::Error(e.to_string()));
                        }
                    }
                    VpnCommand::ListConfigs => {
                        if let Err(e) = self.handle_list_configs().await {
                            self.send_message(VpnMessage::Error(e.to_string()));
                        }
                    }
                    VpnCommand::ImportConfig(path, name) => {
                        if let Err(e) = self.handle_import_config(&path, &name).await {
                            self.send_message(VpnMessage::Error(e.to_string()));
                        }
                    }
                    VpnCommand::RemoveConfig(config_path) => {
                        if let Err(e) = self.handle_remove_config(&config_path).await {
                            self.send_message(VpnMessage::Error(e.to_string()));
                        }
                    }
                    VpnCommand::StartLogging => {
                        *self.logging_active.lock().unwrap() = true;
                        log_monitoring = true;
                        self.send_message(VpnMessage::LogMessage("Started real-time logging".to_string()));
                    }
                    VpnCommand::StopLogging => {
                        *self.logging_active.lock().unwrap() = false;
                        log_monitoring = false;
                        self.send_message(VpnMessage::LogMessage("Stopped real-time logging".to_string()));
                    }
                    VpnCommand::Shutdown => {
                        self.send_message(VpnMessage::LogMessage("Shutting down manager...".to_string()));
                        break;
                    }
                }
            }

            // Monitor connection if active
            if monitoring {
                if let Err(e) = self.update_status().await {
                    self.send_message(VpnMessage::Error(e.to_string()));
                }

                // Get session stats if connected
                if matches!(self.get_status(), ConnectionStatus::Connected { .. }) {
                    let _ = self.handle_get_session_stats().await;
                }

                // Check if we're still connected
                let status = self.get_status();
                if matches!(status, ConnectionStatus::Disconnected | ConnectionStatus::Error(_)) {
                    monitoring = false;
                }
            }

            sleep(Duration::from_millis(2000)).await;
        }
    }

    // ... (rest of the implementation methods remain


    async fn handle_list_sessions(&self) -> Result<()> {
        let output = TokioCommand::new("openvpn3")
            .args(&["sessions-list"])
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow!("Failed to list sessions"));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        let sessions = self.parse_sessions_list(&output_str);

        *self.sessions.lock().unwrap() = sessions.clone();
        self.send_message(VpnMessage::SessionsList(sessions));

        Ok(())
    }

    async fn handle_list_configs(&self) -> Result<()> {
        let output = TokioCommand::new("openvpn3")
            .args(&["configs-list"])
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow!("Failed to list configs"));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        let configs = self.parse_configs_list(&output_str);

        *self.configs.lock().unwrap() = configs.clone();
        self.send_message(VpnMessage::ConfigsList(configs));

        Ok(())
    }

    async fn handle_import_config(&self, file_path: &str, name: &str) -> Result<()> {
        let output = TokioCommand::new("openvpn3")
            .args(&["config-import", "--config", file_path, "--name", name])
            .output()
            .await?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to import config: {}", error));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        self.send_message(VpnMessage::ConfigImported(output_str.to_string()));

        // Refresh configs list
        let _ = self.handle_list_configs().await;

        Ok(())
    }

    async fn handle_remove_config(&self, config_path: &str) -> Result<()> {
        let output = TokioCommand::new("openvpn3")
            .args(&["config-remove", "--path", config_path])
            .output()
            .await?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to remove config: {}", error));
        }

        // Refresh configs list
        let _ = self.handle_list_configs().await;

        Ok(())
    }

    async fn handle_get_session_stats(&self) -> Result<()> {
        let session = {
            let session_guard = self.current_session.lock().unwrap();
            session_guard.clone()
        };

        if let Some(session_path) = session {
            let output = TokioCommand::new("openvpn3")
                .args(&["session-stats", "--session-path", &session_path])
                .output()
                .await?;

            if output.status.success() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                if let Some(stats) = self.parse_session_stats(&output_str) {
                    *self.stats.lock().unwrap() = Some(stats.clone());
                    self.send_message(VpnMessage::SessionStats(stats));
                }
            }
        }

        Ok(())
    }

    async fn handle_log_monitoring(&self) -> Result<()> {
        // Note: This would need to be implemented as a separate long-running process
        // For now, we'll simulate it by checking session logs
        Ok(())
    }

    fn parse_sessions_list(&self, output: &str) -> Vec<SessionInfo> {
        let mut sessions = Vec::new();
        let mut session_path = String::new();
        let mut config_name = String::new();
        let mut status = String::new();
        let mut created = String::new();
        let mut owner = String::new();

        for line in output.lines() {
            let line = line.trim();
            if line.starts_with("Session path:") {
                session_path = line.split(':').nth(1).unwrap_or("").trim().to_string();
            } else if line.starts_with("Config name:") {
                config_name = line.split(':').nth(1).unwrap_or("").trim().to_string();
            } else if line.starts_with("Status:") {
                status = line.split(':').nth(1).unwrap_or("").trim().to_string();
            } else if line.starts_with("Created:") {
                created = line.split(':').nth(1).unwrap_or("").trim().to_string();
            } else if line.starts_with("Owner:") {
                owner = line.split(':').nth(1).unwrap_or("").trim().to_string();

                // Complete session info
                if !session_path.is_empty() {
                    sessions.push(SessionInfo {
                        session_path: session_path.clone(),
                        config_name: config_name.clone(),
                        status: status.clone(),
                        created: created.clone(),
                        owner: owner.clone(),
                    });

                    // Reset for next session
                    session_path.clear();
                    config_name.clear();
                    status.clear();
                    created.clear();
                    owner.clear();
                }
            }
        }

        sessions
    }

    fn parse_configs_list(&self, output: &str) -> Vec<ConfigProfile> {
        let mut configs = Vec::new();
        let mut config_path = String::new();
        let mut name = String::new();
        let mut import_time = String::new();
        let mut owner = String::new();

        for line in output.lines() {
            let line = line.trim();
            if line.starts_with("Configuration path:") {
                config_path = line.split(':').nth(1).unwrap_or("").trim().to_string();
            } else if line.starts_with("Configuration name:") {
                name = line.split(':').nth(1).unwrap_or("").trim().to_string();
            } else if line.starts_with("Imported:") {
                import_time = line.split(':').nth(1).unwrap_or("").trim().to_string();
            } else if line.starts_with("Owner:") {
                owner = line.split(':').nth(1).unwrap_or("").trim().to_string();

                // Complete config info
                if !config_path.is_empty() {
                    configs.push(ConfigProfile {
                        path: config_path.clone(),
                        name: name.clone(),
                        import_time: import_time.clone(),
                        owner: owner.clone(),
                        valid: true, // We assume it's valid if it's listed
                    });

                    // Reset for next config
                    config_path.clear();
                    name.clear();
                    import_time.clear();
                    owner.clear();
                }
            }
        }

        configs
    }

    fn parse_session_stats(&self, output: &str) -> Option<SessionStats> {
        let mut bytes_in = 0;
        let mut bytes_out = 0;
        let mut packets_in = 0;
        let mut packets_out = 0;
        let mut connected_since = String::new();
        let mut virtual_ip = String::new();
        let mut remote_ip = String::new();

        for line in output.lines() {
            let line = line.trim();
            if line.starts_with("Bytes received:") {
                bytes_in = line.split(':').nth(1)
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
            } else if line.starts_with("Bytes sent:") {
                bytes_out = line.split(':').nth(1)
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
            } else if line.starts_with("Packets received:") {
                packets_in = line.split(':').nth(1)
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
            } else if line.starts_with("Packets sent:") {
                packets_out = line.split(':').nth(1)
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
            } else if line.starts_with("Connected:") {
                connected_since = line.split(':').nth(1).unwrap_or("").trim().to_string();
            } else if line.starts_with("Virtual IP:") {
                virtual_ip = line.split(':').nth(1).unwrap_or("").trim().to_string();
            } else if line.starts_with("Remote IP:") {
                remote_ip = line.split(':').nth(1).unwrap_or("").trim().to_string();
            }
        }

        Some(SessionStats {
            bytes_in,
            bytes_out,
            packets_in,
            packets_out,
            connected_since,
            virtual_ip,
            remote_ip,
        })
    }

    // ... existing methods (handle_connect, handle_disconnect, etc.)

    async fn handle_connect(&self, config_path: &str) -> Result<()> {
        let current_status = self.get_status();
        if matches!(current_status, ConnectionStatus::Connected { .. }) {
            return Err(anyhow!("Already connected"));
        }

        self.set_status(ConnectionStatus::Connecting);
        self.send_message(VpnMessage::LogMessage("Starting connection...".to_string()));

        // Use config path directly if it starts with '/net/openvpn/v3/configuration/'
        // Otherwise, assume it's a file path and use session-start with --config
        let output = if config_path.starts_with("/net/openvpn/v3/configuration/") {
            TokioCommand::new("openvpn3")
                .args(&["session-start", "--config-path", config_path])
                .output()
                .await?
        } else {
            TokioCommand::new("openvpn3")
                .args(&["session-start", "--config", config_path])
                .output()
                .await?
        };

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            self.set_status(ConnectionStatus::Error(error.to_string()));
            return Err(anyhow!("Failed to start session: {}", error));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        self.send_message(VpnMessage::LogMessage(output_str.to_string()));

        if let Some(session_line) = output_str.lines()
            .find(|line| line.contains("Session path:")) {
            let session_path = session_line
                .split("Session path:")
                .nth(1)
                .unwrap_or("")
                .trim();
            *self.current_session.lock().unwrap() = Some(session_path.to_string());
            self.send_message(VpnMessage::LogMessage(
                format!("Session started: {}", session_path)
            ));
        }

        Ok(())
    }

    async fn handle_disconnect(&self) -> Result<()> {
        let session = {
            let session_guard = self.current_session.lock().unwrap();
            session_guard.clone()
        };

        if let Some(session_path) = session {
            self.set_status(ConnectionStatus::Disconnecting);
            self.send_message(VpnMessage::LogMessage("Disconnecting...".to_string()));

            let output = TokioCommand::new("openvpn3")
                .args(&["session-manage", "--session-path", &session_path, "--disconnect"])
                .output()
                .await?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("Failed to disconnect: {}", error));
            }

            *self.current_session.lock().unwrap() = None;
            *self.stats.lock().unwrap() = None;
            self.set_status(ConnectionStatus::Disconnected);
            self.send_message(VpnMessage::LogMessage("Disconnected".to_string()));
        }

        Ok(())
    }

    async fn update_status(&self) -> Result<()> {
        let session = {
            let session_guard = self.current_session.lock().unwrap();
            session_guard.clone()
        };

        if let Some(session_path) = session {
            match self.get_session_status(&session_path).await {
                Ok(status) => {
                    let old_status = self.get_status();
                    if !self.status_matches(&old_status, &status) {
                        self.set_status(status);
                    }
                }
                Err(e) => {
                    self.set_status(ConnectionStatus::Error(e.to_string()));
                }
            }
        }

        Ok(())
    }

    async fn get_session_status(&self, session_path: &str) -> Result<ConnectionStatus> {
        let output = TokioCommand::new("openvpn3")
            .args(&["session-stats", "--session-path", session_path])
            .output()
            .await?;

        if !output.status.success() {
            return Ok(ConnectionStatus::Error("Failed to get session status".to_string()));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);

        if output_str.contains("Connection status: Connected") {
            let ip = self.extract_field(&output_str, "Virtual IP:")
                .unwrap_or_else(|| "Unknown".to_string());
            let duration = self.extract_field(&output_str, "Connected:")
                .unwrap_or_else(|| "Unknown".to_string());

            Ok(ConnectionStatus::Connected { ip, duration })
        } else if output_str.contains("Connecting") {
            Ok(ConnectionStatus::Connecting)
        } else {
            Ok(ConnectionStatus::Disconnected)
        }
    }

    fn extract_field(&self, text: &str, field: &str) -> Option<String> {
        text.lines()
            .find(|line| line.contains(field))
            .and_then(|line| line.split(':').nth(1))
            .map(|s| s.trim().to_string())
    }

    fn set_status(&self, status: ConnectionStatus) {
        *self.status.lock().unwrap() = status.clone();
        self.send_message(VpnMessage::StatusUpdate(status));
    }

    fn send_message(&self, message: VpnMessage) {
        let _ = self.message_tx.send(message);
    }

    fn status_matches(&self, old: &ConnectionStatus, new: &ConnectionStatus) -> bool {
        match (old, new) {
            (ConnectionStatus::Connected { .. }, ConnectionStatus::Connected { .. }) => true,
            (ConnectionStatus::Disconnected, ConnectionStatus::Disconnected) => true,
            (ConnectionStatus::Connecting, ConnectionStatus::Connecting) => true,
            (ConnectionStatus::Disconnecting, ConnectionStatus::Disconnecting) => true,
            (ConnectionStatus::Error(_), ConnectionStatus::Error(_)) => true,
            _ => false,
        }
    }
}
