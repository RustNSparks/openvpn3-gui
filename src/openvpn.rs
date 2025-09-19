// src/openvpn.rs (Complete implementation with Error variant)
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command as TokioCommand};
use tokio::time::{Duration, sleep};

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected {
        ip: String,
        duration: String,
    },
    Disconnecting,
    Error(String),
    AuthenticationRequired {
        session_path: String,
        prompt: String,
    },
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
    LogMessage(String), // For general logging
    Error(String),      // For critical errors that need user attention
    SessionsList(Vec<SessionInfo>),
    SessionStats(SessionStats),
    ConfigsList(Vec<ConfigProfile>),
    ConfigImported(String),
    VpnCliVersion(String),
    ConfigDumped {
        name: String,
        content: String,
    },
    AuthenticationPrompt {
        session_path: String,
        prompt: String,
    },
}

#[derive(Debug)]
pub enum VpnCommand {
    Connect(String),
    Disconnect,
    ListSessions,
    GetSessionStats,
    ListConfigs,
    ImportConfig(String, String),
    RemoveConfig(String),
    GetStatusUpdate,
    Shutdown,
    EnableManagerLogBuffer,
    DisableManagerLogBuffer,
    StartLiveVpnLogs,
    StopLiveVpnLogs,
    GetVpnCliVersion,
    DumpConfig(String),
    SubmitAuthentication {
        session_path: String,
        response: String,
    },
}

pub struct OpenVPN3Manager {
    status: Arc<Mutex<ConnectionStatus>>,
    current_session_path: Arc<Mutex<Option<String>>>,
    stats: Arc<Mutex<Option<SessionStats>>>,
    sessions: Arc<Mutex<Vec<SessionInfo>>>,
    configs: Arc<Mutex<Vec<ConfigProfile>>>,
    command_tx: mpsc::Sender<VpnCommand>,
    message_tx: mpsc::Sender<VpnMessage>,
    manager_logging_active: Arc<Mutex<bool>>,
    live_log_process: Arc<Mutex<Option<Child>>>,
}

unsafe impl Send for OpenVPN3Manager {}

impl Clone for OpenVPN3Manager {
    fn clone(&self) -> Self {
        Self {
            status: Arc::clone(&self.status),
            current_session_path: Arc::clone(&self.current_session_path),
            stats: Arc::clone(&self.stats),
            sessions: Arc::clone(&self.sessions),
            configs: Arc::clone(&self.configs),
            command_tx: self.command_tx.clone(),
            message_tx: self.message_tx.clone(),
            manager_logging_active: Arc::clone(&self.manager_logging_active),
            live_log_process: Arc::clone(&self.live_log_process),
        }
    }
}

impl OpenVPN3Manager {
    pub fn new(message_tx: mpsc::Sender<VpnMessage>) -> (Self, mpsc::Receiver<VpnCommand>) {
        let (command_tx, command_rx) = mpsc::channel();
        let manager = Self {
            status: Arc::new(Mutex::new(ConnectionStatus::Disconnected)),
            current_session_path: Arc::new(Mutex::new(None)),
            stats: Arc::new(Mutex::new(None)),
            sessions: Arc::new(Mutex::new(vec![])),
            configs: Arc::new(Mutex::new(vec![])),
            command_tx,
            message_tx,
            manager_logging_active: Arc::new(Mutex::new(true)),
            live_log_process: Arc::new(Mutex::new(None)),
        };
        (manager, command_rx)
    }

    // --- Public methods to send commands to the manager ---
    pub fn connect(&self, config_identifier: String) -> Result<()> {
        self.command_tx
            .send(VpnCommand::Connect(config_identifier))?;
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

    pub fn remove_config(&self, config_identifier: String) -> Result<()> {
        self.command_tx
            .send(VpnCommand::RemoveConfig(config_identifier))?;
        Ok(())
    }

    pub fn get_status_update(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::GetStatusUpdate)?;
        Ok(())
    }

    pub fn enable_manager_log_buffer(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::EnableManagerLogBuffer)?;
        Ok(())
    }

    pub fn disable_manager_log_buffer(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::DisableManagerLogBuffer)?;
        Ok(())
    }

    pub fn start_live_vpn_logs(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::StartLiveVpnLogs)?;
        Ok(())
    }

    pub fn stop_live_vpn_logs(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::StopLiveVpnLogs)?;
        Ok(())
    }

    pub fn get_vpn_cli_version(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::GetVpnCliVersion)?;
        Ok(())
    }

    pub fn dump_config(&self, config_identifier: String) -> Result<()> {
        self.command_tx
            .send(VpnCommand::DumpConfig(config_identifier))?;
        Ok(())
    }

    pub fn submit_authentication(&self, session_path: String, response: String) -> Result<()> {
        self.command_tx.send(VpnCommand::SubmitAuthentication {
            session_path,
            response,
        })?;
        Ok(())
    }

    pub fn shutdown(&self) -> Result<()> {
        self.command_tx.send(VpnCommand::Shutdown)?;
        Ok(())
    }

    pub fn get_status(&self) -> ConnectionStatus {
        self.status.lock().unwrap().clone()
    }

    // --- Manager's main loop (runs in a separate tokio task) ---
    pub async fn run_manager_loop(&self, command_rx: mpsc::Receiver<VpnCommand>) {
        let mut connection_monitoring_active = false;

        // Initial data load with error handling
        if let Err(e) = self.handle_list_configs().await {
            self.send_error(format!("Failed to load initial configs: {}", e));
        }
        if let Err(e) = self.handle_list_sessions().await {
            self.send_error(format!("Failed to load initial sessions: {}", e));
        }
        if let Err(e) = self.handle_get_vpn_cli_version().await {
            self.send_error(format!("Failed to get OpenVPN3 CLI version: {}", e));
        }

        self.log_manager_message("Manager loop started.".to_string());

        loop {
            if let Ok(command) = command_rx.try_recv() {
                match command {
                    VpnCommand::Connect(config_id) => {
                        if let Err(e) = self.handle_connect(&config_id).await {
                            self.send_error(format!(
                                "Connection failed for '{}': {}",
                                config_id, e
                            ));
                            self.set_status(ConnectionStatus::Error(e.to_string()));
                        } else {
                            connection_monitoring_active = true;
                        }
                    }
                    VpnCommand::Disconnect => {
                        if let Err(e) = self.handle_disconnect().await {
                            self.send_error(format!("Disconnect failed: {}", e));
                        }
                        connection_monitoring_active = false;
                    }
                    VpnCommand::GetStatusUpdate => {
                        if let Err(e) = self.update_connection_status().await {
                            self.send_error(format!("Status update failed: {}", e));
                        }
                    }
                    VpnCommand::ListSessions => {
                        if let Err(e) = self.handle_list_sessions().await {
                            self.send_error(format!("Failed to list sessions: {}", e));
                        }
                    }
                    VpnCommand::GetSessionStats => {
                        if let Err(e) = self.handle_get_session_stats().await {
                            self.send_error(format!("Failed to get session stats: {}", e));
                        }
                    }
                    VpnCommand::ListConfigs => {
                        if let Err(e) = self.handle_list_configs().await {
                            self.send_error(format!("Failed to list configs: {}", e));
                        }
                    }
                    VpnCommand::ImportConfig(path, name) => {
                        if let Err(e) = self.handle_import_config(&path, &name).await {
                            self.send_error(format!("Failed to import config '{}': {}", name, e));
                        }
                    }
                    VpnCommand::RemoveConfig(config_id) => {
                        if let Err(e) = self.handle_remove_config(&config_id).await {
                            self.send_error(format!(
                                "Failed to remove config '{}': {}",
                                config_id, e
                            ));
                        }
                    }
                    VpnCommand::EnableManagerLogBuffer => {
                        *self.manager_logging_active.lock().unwrap() = true;
                        self.log_manager_message("Manager log buffer enabled.".to_string());
                    }
                    VpnCommand::DisableManagerLogBuffer => {
                        *self.manager_logging_active.lock().unwrap() = false;
                    }
                    VpnCommand::StartLiveVpnLogs => {
                        if let Err(e) = self.handle_start_live_vpn_logs().await {
                            self.send_error(format!("Failed to start live VPN logs: {}", e));
                        }
                    }
                    VpnCommand::StopLiveVpnLogs => {
                        if let Err(e) = self.handle_stop_live_vpn_logs().await {
                            self.send_error(format!("Failed to stop live VPN logs: {}", e));
                        }
                    }
                    VpnCommand::GetVpnCliVersion => {
                        if let Err(e) = self.handle_get_vpn_cli_version().await {
                            self.send_error(format!("Failed to get VPN CLI version: {}", e));
                        }
                    }
                    VpnCommand::DumpConfig(config_id) => {
                        if let Err(e) = self.handle_dump_config(&config_id).await {
                            self.send_error(format!(
                                "Failed to dump config '{}': {}",
                                config_id, e
                            ));
                        }
                    }
                    VpnCommand::SubmitAuthentication {
                        session_path,
                        response,
                    } => {
                        if let Err(e) = self
                            .handle_submit_authentication(&session_path, &response)
                            .await
                        {
                            self.send_error(format!("Authentication failed: {}", e));
                        }
                    }
                    VpnCommand::Shutdown => {
                        self.log_manager_message("Shutdown command received.".to_string());
                        let _ = self.handle_stop_live_vpn_logs().await;
                        break;
                    }
                }
            }

            // Periodic monitoring with error handling
            if connection_monitoring_active {
                if let Err(e) = self.update_connection_status().await {
                    self.log_manager_message(format!("Status update error: {}", e));
                }

                if matches!(self.get_status(), ConnectionStatus::Connected { .. })
                    && let Err(e) = self.handle_get_session_stats().await
                {
                    self.log_manager_message(format!("Stats update error: {}", e));
                }

                let current_status = self.get_status();
                if matches!(
                    current_status,
                    ConnectionStatus::Disconnected | ConnectionStatus::Error(_)
                ) {
                    connection_monitoring_active = false;
                    self.log_manager_message("Disabling active monitoring.".to_string());
                }
            }
            sleep(Duration::from_secs(2)).await;
        }
        self.log_manager_message("Manager loop terminated.".to_string());
    }

    // --- Helper methods for messaging and status ---
    fn send_error(&self, error_msg: String) {
        // Send critical errors as Error messages
        self.send_message_to_ui(VpnMessage::Error(error_msg.clone()));
        // Also log for debugging if logging is active
        if *self.manager_logging_active.lock().unwrap() {
            self.send_message_to_ui(VpnMessage::LogMessage(format!(
                "[MANAGER ERROR] {}",
                error_msg
            )));
        }
    }

    fn set_status(&self, new_status: ConnectionStatus) {
        let mut status_guard = self.status.lock().unwrap();
        *status_guard = new_status.clone();
        drop(status_guard);
        self.send_message_to_ui(VpnMessage::StatusUpdate(new_status));
    }

    fn log_manager_message(&self, message: String) {
        if *self.manager_logging_active.lock().unwrap() {
            self.send_message_to_ui(VpnMessage::LogMessage(format!("[MANAGER] {}", message)));
        }
    }

    fn send_message_to_ui(&self, message: VpnMessage) {
        if let Err(e) = self.message_tx.send(message) {
            eprintln!("OpenVPN3Manager: Failed to send message to UI: {}", e);
        }
    }

    // --- Command Handlers (called by run_manager_loop) ---
    async fn handle_connect(&self, config_identifier: &str) -> Result<()> {
        let current_status = self.get_status();
        if !matches!(
            current_status,
            ConnectionStatus::Disconnected | ConnectionStatus::Error(_)
        ) {
            return Err(anyhow!(
                "Cannot connect: Already connected or in an intermediate state ({:?})",
                current_status
            ));
        }

        self.set_status(ConnectionStatus::Connecting);
        self.log_manager_message(format!(
            "Attempting to connect using: {}",
            config_identifier
        ));

        let mut cmd = TokioCommand::new("openvpn3");
        cmd.arg("session-start");
        if config_identifier.starts_with("/net/openvpn/v3/configuration/") {
            cmd.args(["--config-path", config_identifier]);
        } else {
            cmd.args(["--config", config_identifier]);
        }

        let output = cmd.output().await?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr).to_string();
            if error_msg.to_lowercase().contains("authentication failed")
                || error_msg.to_lowercase().contains("auth-user-pass")
            {
                let stdout_str = String::from_utf8_lossy(&output.stdout);
                if let Some(session_path) = self.extract_session_path_from_output(&stdout_str) {
                    self.set_status(ConnectionStatus::AuthenticationRequired {
                        session_path: session_path.clone(),
                        prompt: format!(
                            "Authentication required for {}. Details: {}",
                            config_identifier, error_msg
                        ),
                    });
                    self.send_message_to_ui(VpnMessage::AuthenticationPrompt {
                        session_path,
                        prompt: format!(
                            "Credentials needed for {}. Error: {}",
                            config_identifier, error_msg
                        ),
                    });
                    return Ok(());
                }
            }
            self.set_status(ConnectionStatus::Error(error_msg.clone()));
            return Err(anyhow!("Failed to start session: {}", error_msg));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        self.log_manager_message(format!("Session start output: {}", output_str));

        if let Some(session_path) = self.extract_session_path_from_output(&output_str) {
            *self.current_session_path.lock().unwrap() = Some(session_path.clone());
            self.log_manager_message(format!(
                "Session started successfully. Path: {}",
                session_path
            ));
        } else {
            self.log_manager_message("Session start reported success, but session path not found in output. Will rely on status updates.".to_string());
        }

        self.update_connection_status().await?;
        Ok(())
    }

    fn extract_session_path_from_output(&self, output: &str) -> Option<String> {
        output
            .lines()
            .find(|line| line.contains("Session path:"))
            .and_then(|line| line.split("Session path:").nth(1))
            .map(|s| s.trim().to_string())
    }

    async fn handle_disconnect(&self) -> Result<()> {
        let session_path_opt = self.current_session_path.lock().unwrap().clone();

        if let Some(session_path) = session_path_opt {
            self.set_status(ConnectionStatus::Disconnecting);
            self.log_manager_message(format!("Disconnecting session: {}", session_path));

            let output = TokioCommand::new("openvpn3")
                .args([
                    "session-manage",
                    "--session-path",
                    &session_path,
                    "--disconnect",
                ])
                .output()
                .await?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr).to_string();
                self.log_manager_message(format!("Disconnect command failed: {}", error));
            } else {
                self.log_manager_message("Disconnect command successful.".to_string());
            }
        } else {
            self.log_manager_message(
                "Disconnect called but no active session path found.".to_string(),
            );
        }

        *self.current_session_path.lock().unwrap() = None;
        *self.stats.lock().unwrap() = None;
        self.set_status(ConnectionStatus::Disconnected);
        Ok(())
    }

    async fn update_connection_status(&self) -> Result<()> {
        // We now primarily use `sessions-list` as it's more reliable for status.
        let list_output = TokioCommand::new("openvpn3")
            .arg("sessions-list")
            .output()
            .await?;

        if !list_output.status.success() {
            // If the command fails, assume disconnected
            self.set_status(ConnectionStatus::Disconnected);
            return Err(anyhow!("Failed to list sessions for status update."));
        }

        let list_output_str = String::from_utf8_lossy(&list_output.stdout);
        let sessions = self.parse_sessions_list(&list_output_str);
        let session_path_opt = self.current_session_path.lock().unwrap().clone();

        if let Some(session_path) = session_path_opt {
            if let Some(active_session) = sessions.iter().find(|s| s.session_path == session_path) {
                // Check for the specific connected status message you provided
                if active_session
                    .status
                    .contains("Connection, Client connected")
                {
                    let new_status = ConnectionStatus::Connected {
                        // This info isn't in your output, so we provide placeholders
                        ip: "Assigned".to_string(),
                        duration: active_session.created.clone(),
                    };
                    if self.status_changed(&self.get_status(), &new_status) {
                        self.set_status(new_status);
                    }
                } else {
                    // If the status is not "Client connected", we assume it's still connecting
                    self.set_status(ConnectionStatus::Connecting);
                }
            } else {
                // Session path is known, but it's not in the list anymore, so it's disconnected
                *self.current_session_path.lock().unwrap() = None;
                self.set_status(ConnectionStatus::Disconnected);
            }
        } else {
            // No session path is being tracked, so we are disconnected.
            self.set_status(ConnectionStatus::Disconnected);
        }
        Ok(())
    }

    fn parse_status_from_session_output(
        &self,
        output_str: &str,
        session_path: &str,
    ) -> ConnectionStatus {
        let lower_output = output_str.to_lowercase();

        // Prioritize checking for a Virtual IP, as it's a strong indicator of a connection.
        if let Some(ip) = self.extract_field(output_str, "Virtual IP:") {
            let duration = self
                .extract_field(output_str, "Connected since:")
                .or_else(|| self.extract_field(output_str, "Duration:"))
                .unwrap_or_else(|| "N/A".to_string());
            return ConnectionStatus::Connected { ip, duration };
        }

        if lower_output.contains("status: connected")
            || lower_output.contains("connection initiated")
        {
            let ip = self
                .extract_field(output_str, "Virtual IP:")
                .unwrap_or_else(|| "N/A".to_string());
            let duration = self
                .extract_field(output_str, "Connected since:")
                .or_else(|| self.extract_field(output_str, "Duration:"))
                .unwrap_or_else(|| "N/A".to_string());
            ConnectionStatus::Connected { ip, duration }
        } else if lower_output.contains("status: connecting") {
            ConnectionStatus::Connecting
        } else if lower_output.contains("status: authenticating")
            || lower_output.contains("auth_pending")
        {
            ConnectionStatus::AuthenticationRequired {
                session_path: session_path.to_string(),
                prompt: "Authentication is pending. Please provide credentials.".to_string(),
            }
        } else if lower_output.contains("status: disconnected")
            || lower_output.contains("status: exited")
        {
            ConnectionStatus::Disconnected
        } else {
            self.log_manager_message(format!(
                "Could not determine clear status from session output for {}. Output: {}",
                session_path, output_str
            ));
            ConnectionStatus::Connecting
        }
    }

    async fn handle_list_sessions(&self) -> Result<()> {
        let output = TokioCommand::new("openvpn3")
            .arg("sessions-list")
            .output()
            .await?;
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to list sessions: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let output_str = String::from_utf8_lossy(&output.stdout);
        let sessions = self.parse_sessions_list(&output_str);
        *self.sessions.lock().unwrap() = sessions.clone();
        self.send_message_to_ui(VpnMessage::SessionsList(sessions));
        Ok(())
    }

    async fn handle_get_session_stats(&self) -> Result<()> {
        let session_path_opt = self.current_session_path.lock().unwrap().clone();
        if let Some(session_path) = session_path_opt {
            let output = TokioCommand::new("openvpn3")
                .args(["session-stats", "--session-path", &session_path])
                .output()
                .await?;

            if output.status.success() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                if let Some(stats) = self.parse_session_stats(&output_str) {
                    *self.stats.lock().unwrap() = Some(stats.clone());
                    self.send_message_to_ui(VpnMessage::SessionStats(stats));
                } else {
                    self.log_manager_message(format!(
                        "Could not parse session stats from output: {}",
                        output_str
                    ));
                }
            } else {
                return Err(anyhow!(
                    "Failed to get session stats ({}): {}",
                    session_path,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        } else {
            self.log_manager_message("Get session stats called but no active session.".to_string());
            *self.stats.lock().unwrap() = None;
        }
        Ok(())
    }

    async fn handle_list_configs(&self) -> Result<()> {
        let output = TokioCommand::new("openvpn3")
            .arg("configs-list")
            .output()
            .await?;
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to list configs: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let output_str = String::from_utf8_lossy(&output.stdout);
        let configs = self.parse_configs_list(&output_str);
        *self.configs.lock().unwrap() = configs.clone();
        self.send_message_to_ui(VpnMessage::ConfigsList(configs));
        Ok(())
    }

    async fn handle_import_config(&self, file_path: &str, name: &str) -> Result<()> {
        let output = TokioCommand::new("openvpn3")
            .args([
                "config-import",
                "--config",
                file_path,
                "--name",
                name,
                "--persistent",
            ])
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to import config '{}': {}",
                name,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let output_str = String::from_utf8_lossy(&output.stdout).to_string();
        self.send_message_to_ui(VpnMessage::ConfigImported(output_str.clone()));
        self.log_manager_message(format!("Config imported: {}", output_str));
        self.handle_list_configs().await?;
        Ok(())
    }

    async fn handle_remove_config(&self, config_identifier: &str) -> Result<()> {
        let mut cmd = TokioCommand::new("openvpn3");
        cmd.arg("config-remove");
        if config_identifier.starts_with("/net/openvpn/v3/configuration/") {
            cmd.args(["--path", config_identifier]);
        } else {
            cmd.args(["--name", config_identifier]);
        }

        let output = cmd.output().await?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to remove config '{}': {}",
                config_identifier,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        self.log_manager_message(format!("Config removed: {}", config_identifier));
        self.handle_list_configs().await?;
        Ok(())
    }

    async fn handle_start_live_vpn_logs(&self) -> Result<()> {
        self.handle_stop_live_vpn_logs().await?;

        self.log_manager_message("Starting live VPN log streaming...".to_string());
        let mut cmd_process = TokioCommand::new("openvpn3")
            .arg("log")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = cmd_process
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to capture stdout from 'openvpn3 log'"))?;
        let stderr = cmd_process
            .stderr
            .take()
            .ok_or_else(|| anyhow!("Failed to capture stderr from 'openvpn3 log'"))?;

        {
            let mut guard = self.live_log_process.lock().unwrap();
            *guard = Some(cmd_process);
        }

        let message_tx_stdout = self.message_tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Err(e) = message_tx_stdout.send(VpnMessage::LogMessage(line)) {
                    eprintln!("Failed to send stdout log message to UI: {}", e);
                }
            }
        });

        let message_tx_stderr = self.message_tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Err(e) = message_tx_stderr.send(VpnMessage::LogMessage(format!(
                    "[openvpn3 log err] {}",
                    line
                ))) {
                    eprintln!("Failed to send stderr log message to UI: {}", e);
                }
            }
        });
        Ok(())
    }

    async fn handle_stop_live_vpn_logs(&self) -> Result<()> {
        let child_opt = {
            let mut guard = self.live_log_process.lock().unwrap();
            guard.take()
        };

        if let Some(mut child) = child_opt {
            self.log_manager_message("Stopping live VPN log streaming...".to_string());
            child
                .kill()
                .await
                .map_err(|e| anyhow!("Failed to kill 'openvpn3 log' process: {}", e))?;
            self.log_manager_message("Live VPN log streaming stopped.".to_string());
        }
        Ok(())
    }

    async fn handle_get_vpn_cli_version(&self) -> Result<()> {
        let output = TokioCommand::new("openvpn3")
            .arg("version")
            .output()
            .await?;
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to get VPN CLI version: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let version_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        self.send_message_to_ui(VpnMessage::VpnCliVersion(version_str));
        Ok(())
    }

    async fn handle_dump_config(&self, config_identifier: &str) -> Result<()> {
        let mut cmd = TokioCommand::new("openvpn3");
        cmd.arg("config-dump"); // Correct command name
        if config_identifier.starts_with("/net/openvpn/v3/configuration/") {
            // Using --path for D-Bus paths is correct
            cmd.args(["--path", config_identifier]);
        } else {
            // CORRECTED: Using --config for configuration names, as per your man page
            cmd.args(["--config", config_identifier]);
        }

        let output = cmd.output().await?;
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to dump config '{}': {}",
                config_identifier,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let content = String::from_utf8_lossy(&output.stdout).to_string();

        // This logic to find the display name remains correct
        let name = if config_identifier.starts_with("/net/openvpn/v3/configuration/") {
            self.configs
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.path == config_identifier)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| config_identifier.to_string())
        } else {
            config_identifier.to_string()
        };
        self.send_message_to_ui(VpnMessage::ConfigDumped { name, content });
        Ok(())
    }
    async fn handle_submit_authentication(&self, session_path: &str, response: &str) -> Result<()> {
        self.log_manager_message(format!(
            "Submitting authentication for session: {}",
            session_path
        ));

        let mut child = TokioCommand::new("openvpn3")
            .args([
                "session-manage",
                "--session-path",
                session_path,
                "--password-prompt",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let mut stdin = stdin;
            let _ = stdin.write_all(response.as_bytes()).await;
            let _ = stdin.write_all(b"\n").await;
            let _ = stdin.shutdown().await;
        }

        let output = child.wait_with_output().await?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr).to_string();
            self.set_status(ConnectionStatus::Error(format!(
                "Authentication failed: {}",
                error_msg
            )));
            return Err(anyhow!(
                "Authentication submission failed for {}: {}",
                session_path,
                error_msg
            ));
        }

        self.log_manager_message(format!(
            "Authentication submitted for {}. Output: {}",
            session_path,
            String::from_utf8_lossy(&output.stdout)
        ));
        self.set_status(ConnectionStatus::Connecting);
        self.update_connection_status().await?;
        Ok(())
    }

    // --- Parsing Helpers ---
    fn parse_sessions_list(&self, output: &str) -> Vec<SessionInfo> {
        let mut sessions = Vec::new();
        let mut current_session: Option<SessionInfo> = None;

        for line in output.lines() {
            let trimmed_line = line.trim();
            if trimmed_line.starts_with("Path:") {
                if let Some(session) = current_session.take() {
                    sessions.push(session);
                }
                current_session = Some(SessionInfo {
                    session_path: self.extract_value(trimmed_line, "Path:"),
                    config_name: String::new(),
                    status: String::new(),
                    created: String::new(),
                    owner: String::new(),
                });
            } else if let Some(session) = current_session.as_mut() {
                if trimmed_line.starts_with("Config name:") {
                    session.config_name = self.extract_value(trimmed_line, "Config name:");
                } else if trimmed_line.starts_with("Status:") {
                    session.status = self.extract_value(trimmed_line, "Status:");
                } else if trimmed_line.starts_with("Created:") {
                    session.created = self.extract_value(trimmed_line, "Created:");
                } else if trimmed_line.starts_with("Owner:") {
                    session.owner = self.extract_value(trimmed_line, "Owner:");
                }
            }
        }
        if let Some(session) = current_session.take() {
            sessions.push(session);
        }
        sessions
    }

    fn parse_configs_list(&self, output: &str) -> Vec<ConfigProfile> {
        let mut configs = Vec::new();
        let lines = output.lines().skip(2); // Skip header lines

        for line in lines {
            let trimmed_line = line.trim();
            if trimmed_line.is_empty() || trimmed_line.starts_with("---") {
                continue;
            }

            let parts: Vec<&str> = trimmed_line.split_whitespace().collect();
            if let Some(name) = parts.first() {
                configs.push(ConfigProfile {
                    name: name.to_string(),
                    // Path is not available in this output format, so we'll use the name as a placeholder
                    path: name.to_string(),
                    // Other fields are also not available, so we'll use default values
                    import_time: "N/A".to_string(),
                    owner: "N/A".to_string(),
                    valid: true, // Assuming configs listed are valid
                });
            }
        }
        configs
    }

    fn extract_value(&self, line: &str, key: &str) -> String {
        line.split_once(key)
            .map(|(_, val)| val.trim().to_string())
            .unwrap_or_default()
    }

    fn parse_session_stats(&self, output: &str) -> Option<SessionStats> {
        let mut stats = SessionStats {
            bytes_in: 0,
            bytes_out: 0,
            packets_in: 0,
            packets_out: 0,
            connected_since: "N/A".to_string(),
            virtual_ip: "N/A".to_string(),
            remote_ip: "N/A".to_string(),
        };
        let mut found_any = false;

        for line in output.lines() {
            let line = line.trim();

            // Parse connection statistics format (BYTES_IN, BYTES_OUT, etc.)
            if line.contains("BYTES_IN") {
                if let Some(val) = self.extract_dots_value(line) {
                    stats.bytes_in = val.parse().unwrap_or(0);
                    found_any = true;
                }
            } else if line.contains("BYTES_OUT") {
                if let Some(val) = self.extract_dots_value(line) {
                    stats.bytes_out = val.parse().unwrap_or(0);
                    found_any = true;
                }
            } else if line.contains("PACKETS_IN") && !line.contains("TUN_") {
                if let Some(val) = self.extract_dots_value(line) {
                    stats.packets_in = val.parse().unwrap_or(0);
                    found_any = true;
                }
            } else if line.contains("PACKETS_OUT")
                && !line.contains("TUN_")
                && let Some(val) = self.extract_dots_value(line)
            {
                stats.packets_out = val.parse().unwrap_or(0);
                found_any = true;
            }
        }

        if found_any { Some(stats) } else { None }
    }

    fn extract_dots_value(&self, line: &str) -> Option<String> {
        // Split by dots pattern - find where multiple dots appear
        if let Some(dots_pos) = line.find("..") {
            let (_, value_part) = line.split_at(dots_pos);
            // Skip all dots and get the value
            let value = value_part.trim_start_matches('.').trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
        None
    }

    fn extract_field(&self, text: &str, field_key: &str) -> Option<String> {
        text.lines()
            .find(|line| line.trim().starts_with(field_key))
            .and_then(|line| line.split(':').nth(1))
            .map(|s| s.trim().to_string())
    }

    fn status_changed(&self, old: &ConnectionStatus, new: &ConnectionStatus) -> bool {
        match (old, new) {
            (
                ConnectionStatus::Connected {
                    ip: o_ip,
                    duration: o_dur,
                },
                ConnectionStatus::Connected {
                    ip: n_ip,
                    duration: n_dur,
                },
            ) => o_ip != n_ip || o_dur != n_dur,
            (ConnectionStatus::Error(o_err), ConnectionStatus::Error(n_err)) => o_err != n_err,
            (
                ConnectionStatus::AuthenticationRequired {
                    session_path: o_p,
                    prompt: o_pr,
                },
                ConnectionStatus::AuthenticationRequired {
                    session_path: n_p,
                    prompt: n_pr,
                },
            ) => o_p != n_p || o_pr != n_pr,
            _ => std::mem::discriminant(old) != std::mem::discriminant(new),
        }
    }
}
