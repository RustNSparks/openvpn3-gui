// src/openvpn.rs (Updated with suggestions)
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command as TokioCommand};
use tokio::time::{sleep, Duration};

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
    }, // New
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
    LogMessage(String), // Can be from manager or streamed from openvpn3 log
    Error(String),
    SessionsList(Vec<SessionInfo>),
    SessionStats(SessionStats),
    ConfigsList(Vec<ConfigProfile>),
    ConfigImported(String),
    VpnCliVersion(String), // New
    ConfigDumped {
        name: String,
        content: String,
    }, // New
    AuthenticationPrompt {
        session_path: String,
        prompt: String,
    }, // New: Specific prompt for UI
}

#[derive(Debug)]
pub enum VpnCommand {
    Connect(String), // Config path or name
    Disconnect,
    ListSessions,
    GetSessionStats, // For current session
    ListConfigs,
    ImportConfig(String, String), // path, name
    RemoveConfig(String),         // config path or name
    GetStatusUpdate,
    Shutdown,

    // Logging related commands
    EnableManagerLogBuffer,  // To enable manager's own log messages
    DisableManagerLogBuffer, // To disable manager's own log messages
    StartLiveVpnLogs,        // To start streaming `openvpn3 log`
    StopLiveVpnLogs,         // To stop streaming `openvpn3 log`

    // New commands
    GetVpnCliVersion,
    DumpConfig(String), // Config name or path
    SubmitAuthentication {
        session_path: String,
        response: String,
    }, // New
}

pub struct OpenVPN3Manager {
    status: Arc<Mutex<ConnectionStatus>>,
    current_session_path: Arc<Mutex<Option<String>>>, // Renamed for clarity
    stats: Arc<Mutex<Option<SessionStats>>>,
    sessions: Arc<Mutex<Vec<SessionInfo>>>,
    configs: Arc<Mutex<Vec<ConfigProfile>>>,
    command_tx: mpsc::Sender<VpnCommand>, // To send commands to the manager's loop
    message_tx: mpsc::Sender<VpnMessage>, // To send messages/updates to the UI
    manager_logging_active: Arc<Mutex<bool>>, // Controls if manager sends its own LogMessages
    live_log_process: Arc<Mutex<Option<Child>>>, // Holds the `openvpn3 log` child process
}

unsafe impl Send for OpenVPN3Manager {}

// Manual Clone implementation because Child is not Clone
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
            manager_logging_active: Arc::new(Mutex::new(true)), // Manager logging enabled by default
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

    // --- Getter for current status ---
    pub fn get_status(&self) -> ConnectionStatus {
        self.status.lock().unwrap().clone()
    }

    // --- Manager's main loop (runs in a separate tokio task) ---
    pub async fn run_manager_loop(&self, mut command_rx: mpsc::Receiver<VpnCommand>) {
        let mut connection_monitoring_active = false;

        // Initial data load
        let _ = self.handle_list_configs().await;
        let _ = self.handle_list_sessions().await;
        let _ = self.handle_get_vpn_cli_version().await; // Get version on startup

        self.log_manager_message("Manager loop started.".to_string());

        loop {
            // Non-blocking command check
            if let Ok(command) = command_rx.try_recv() {
                match command {
                    VpnCommand::Connect(config_id) => {
                        if let Err(e) = self.handle_connect(&config_id).await {
                            self.set_status_and_log_error(e.to_string(), "Connect failed");
                        } else {
                            connection_monitoring_active = true; // Start monitoring after connect attempt
                        }
                    }
                    VpnCommand::Disconnect => {
                        if let Err(e) = self.handle_disconnect().await {
                            self.set_status_and_log_error(e.to_string(), "Disconnect failed");
                        }
                        connection_monitoring_active = false; // Stop monitoring
                    }
                    VpnCommand::GetStatusUpdate => {
                        if let Err(e) = self.update_connection_status().await {
                            self.log_manager_error(e.to_string(), "Status update failed");
                        }
                    }
                    VpnCommand::ListSessions => {
                        if let Err(e) = self.handle_list_sessions().await {
                            self.log_manager_error(e.to_string(), "List sessions failed");
                        }
                    }
                    VpnCommand::GetSessionStats => {
                        if let Err(e) = self.handle_get_session_stats().await {
                            self.log_manager_error(e.to_string(), "Get session stats failed");
                        }
                    }
                    VpnCommand::ListConfigs => {
                        if let Err(e) = self.handle_list_configs().await {
                            self.log_manager_error(e.to_string(), "List configs failed");
                        }
                    }
                    VpnCommand::ImportConfig(path, name) => {
                        if let Err(e) = self.handle_import_config(&path, &name).await {
                            self.log_manager_error(e.to_string(), "Import config failed");
                        }
                    }
                    VpnCommand::RemoveConfig(config_id) => {
                        if let Err(e) = self.handle_remove_config(&config_id).await {
                            self.log_manager_error(e.to_string(), "Remove config failed");
                        }
                    }
                    VpnCommand::EnableManagerLogBuffer => {
                        *self.manager_logging_active.lock().unwrap() = true;
                        self.log_manager_message("Manager log buffer enabled.".to_string());
                    }
                    VpnCommand::DisableManagerLogBuffer => {
                        *self.manager_logging_active.lock().unwrap() = false;
                        // No log message here as it's disabled
                    }
                    VpnCommand::StartLiveVpnLogs => {
                        if let Err(e) = self.handle_start_live_vpn_logs().await {
                            self.log_manager_error(e.to_string(), "Start live VPN logs failed");
                        }
                    }
                    VpnCommand::StopLiveVpnLogs => {
                        if let Err(e) = self.handle_stop_live_vpn_logs().await {
                            self.log_manager_error(e.to_string(), "Stop live VPN logs failed");
                        }
                    }
                    VpnCommand::GetVpnCliVersion => {
                        if let Err(e) = self.handle_get_vpn_cli_version().await {
                            self.log_manager_error(e.to_string(), "Get VPN CLI version failed");
                        }
                    }
                    VpnCommand::DumpConfig(config_id) => {
                        if let Err(e) = self.handle_dump_config(&config_id).await {
                            self.log_manager_error(e.to_string(), "Dump config failed");
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
                            self.log_manager_error(e.to_string(), "Submit authentication failed");
                        }
                    }
                    VpnCommand::Shutdown => {
                        self.log_manager_message(
                            "Shutdown command received. Stopping live logs and exiting loop."
                                .to_string(),
                        );
                        let _ = self.handle_stop_live_vpn_logs().await; // Ensure logs are stopped
                        break; // Exit the manager loop
                    }
                }
            }

            // Periodically monitor connection if active
            if connection_monitoring_active {
                if let Err(e) = self.update_connection_status().await {
                    self.log_manager_error(e.to_string(), "Periodic status update failed");
                }

                // Get session stats if connected
                if matches!(self.get_status(), ConnectionStatus::Connected { .. }) {
                    if let Err(e) = self.handle_get_session_stats().await {
                        self.log_manager_error(e.to_string(), "Periodic session stats failed");
                    }
                }

                // If connection dropped or errored, stop intensive monitoring
                let current_status = self.get_status();
                if matches!(
                    current_status,
                    ConnectionStatus::Disconnected | ConnectionStatus::Error(_)
                ) {
                    connection_monitoring_active = false;
                    self.log_manager_message(
                        "Connection dropped or errored. Disabling active monitoring.".to_string(),
                    );
                }
            }
            sleep(Duration::from_secs(2)).await; // Main loop interval
        }
        self.log_manager_message("Manager loop terminated.".to_string());
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

        // Determine if config_identifier is a path or a name (heuristic)
        // OpenVPN3 CLI is flexible: session-start --config <file_path> OR --config-path <dbus_path>
        // We'll try to intelligently pick one. If it looks like a D-Bus path, use --config-path.
        let mut cmd = TokioCommand::new("openvpn3");
        cmd.arg("session-start");
        if config_identifier.starts_with("/net/openvpn/v3/configuration/") {
            cmd.args(&["--config-path", config_identifier]);
        } else {
            // Could be a name or a file path. `openvpn3 session-start --config` handles both.
            cmd.args(&["--config", config_identifier]);
        }

        let output = cmd.output().await?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr).to_string();
            // Check for common auth required messages (this is a guess, actual messages may vary)
            if error_msg.to_lowercase().contains("authentication failed")
                || error_msg.to_lowercase().contains("auth-user-pass")
            {
                // Try to extract session path if connection was initiated but needs auth
                let stdout_str = String::from_utf8_lossy(&output.stdout);
                if let Some(session_path) = self.extract_session_path_from_output(&stdout_str) {
                    self.set_status(ConnectionStatus::AuthenticationRequired {
                        session_path: session_path.clone(),
                        prompt: format!(
                            "Authentication required for {}. Details: {}",
                            config_identifier, error_msg
                        ),
                    });
                    // Send a specific prompt message to UI
                    self.send_message_to_ui(VpnMessage::AuthenticationPrompt {
                        session_path,
                        prompt: format!(
                            "Credentials needed for {}. Error: {}",
                            config_identifier, error_msg
                        ),
                    });
                    return Ok(()); // Not an error, but requires user action
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
            // Status will be updated to Connected by the monitor
        } else {
            // It's possible session started but path wasn't parsed, or it's an unexpected success message
            self.log_manager_message("Session start reported success, but session path not found in output. Will rely on status updates.".to_string());
        }
        // Trigger an immediate status update
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
                .args(&[
                    "session-manage",
                    "--session-path",
                    &session_path,
                    "--disconnect",
                ])
                .output()
                .await?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr).to_string();
                // Don't set to Error status here, as Disconnected is the goal. Log the error.
                self.log_manager_error(error.clone(), "Disconnect command failed");
                // Fall through to set Disconnected, but the error is logged.
            } else {
                self.log_manager_message("Disconnect command successful.".to_string());
            }
        } else {
            self.log_manager_message(
                "Disconnect called but no active session path found.".to_string(),
            );
        }

        // Reset state regardless of command success, as the intent is to be disconnected.
        *self.current_session_path.lock().unwrap() = None;
        *self.stats.lock().unwrap() = None;
        self.set_status(ConnectionStatus::Disconnected);
        Ok(())
    }

    async fn update_connection_status(&self) -> Result<()> {
        let session_path_opt = self.current_session_path.lock().unwrap().clone();

        if let Some(session_path) = session_path_opt {
            // Fetch full session details which might indicate status
            let output = TokioCommand::new("openvpn3")
                .args(&["session-stats", "--session-path", &session_path]) // session-stats often has status info
                .output()
                .await?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // If session path is invalid (e.g., session ended abruptly), it's effectively disconnected
                if stderr.contains("Invalid session path") || stderr.contains("not found") {
                    self.log_manager_message(format!(
                        "Session {} no longer valid. Marking as Disconnected.",
                        session_path
                    ));
                    *self.current_session_path.lock().unwrap() = None;
                    self.set_status(ConnectionStatus::Disconnected);
                } else {
                    self.set_status(ConnectionStatus::Error(format!(
                        "Failed to get session status for {}: {}",
                        session_path, stderr
                    )));
                }
                return Ok(());
            }

            let output_str = String::from_utf8_lossy(&output.stdout);
            let new_status = self.parse_status_from_session_output(&output_str, &session_path);

            // If status indicates auth is needed and we weren't already in that state
            if let ConnectionStatus::AuthenticationRequired { .. } = new_status {
                if !matches!(
                    self.get_status(),
                    ConnectionStatus::AuthenticationRequired { .. }
                ) {
                    self.set_status(new_status.clone());
                    if let ConnectionStatus::AuthenticationRequired {
                        session_path,
                        prompt,
                    } = new_status
                    {
                        self.send_message_to_ui(VpnMessage::AuthenticationPrompt {
                            session_path,
                            prompt,
                        });
                    }
                }
            } else if self.status_changed(&self.get_status(), &new_status) {
                self.set_status(new_status);
            }
        } else {
            // No current session, ensure status is Disconnected if not already Error
            let current_internal_status = self.get_status();
            if !matches!(
                current_internal_status,
                ConnectionStatus::Disconnected | ConnectionStatus::Error(_)
            ) {
                self.set_status(ConnectionStatus::Disconnected);
            }
        }
        Ok(())
    }

    fn parse_status_from_session_output(
        &self,
        output_str: &str,
        session_path: &str,
    ) -> ConnectionStatus {
        // This parsing needs to be robust and based on actual `openvpn3 session-stats` output
        // or `openvpn3 sessions-list` for the specific session.
        // For now, a simplified version:
        if output_str.to_lowercase().contains("status: connected")
            || output_str.to_lowercase().contains("connection initiated")
        {
            let ip = self
                .extract_field(output_str, "Virtual IP:")
                .unwrap_or_else(|| "N/A".to_string());
            let duration = self
                .extract_field(output_str, "Connected since:")
                .or_else(|| self.extract_field(output_str, "Duration:"))
                .unwrap_or_else(|| "N/A".to_string());
            ConnectionStatus::Connected { ip, duration }
        } else if output_str.to_lowercase().contains("status: connecting") {
            ConnectionStatus::Connecting
        } else if output_str.to_lowercase().contains("status: authenticating")
            || output_str.to_lowercase().contains("auth_pending")
        {
            // This is a guess for auth pending
            ConnectionStatus::AuthenticationRequired {
                session_path: session_path.to_string(),
                prompt: "Authentication is pending. Please provide credentials.".to_string(),
            }
        } else if output_str.to_lowercase().contains("status: disconnected")
            || output_str.to_lowercase().contains("status: exited")
        {
            ConnectionStatus::Disconnected
        } else {
            // If status is unclear from stats, it might be an intermediate state or an issue.
            // Defaulting to connecting if session path exists but status is not clear.
            // A better approach would be to also check `sessions-list`.
            self.log_manager_message(format!(
                "Could not determine clear status from session output for {}. Output: {}",
                session_path, output_str
            ));
            ConnectionStatus::Connecting // Or Error, depending on how strict you want to be
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
                .args(&["session-stats", "--session-path", &session_path])
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
            *self.stats.lock().unwrap() = None; // Clear stats if no session
                                                // Optionally send an empty stats message or a specific "no active session" message
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
            .args(&[
                "config-import",
                "--config",
                file_path,
                "--name",
                name,
                "--persistent",
            ]) // Added --persistent
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
        self.handle_list_configs().await?; // Refresh list
        Ok(())
    }

    async fn handle_remove_config(&self, config_identifier: &str) -> Result<()> {
        // `config-remove` can take `--path <dbus_path>` or `--name <name>`
        // We need to determine which one to use or try to be robust.
        // For simplicity, assuming identifier might be a name first, then try path if it looks like one.
        // A more robust way would be to always list configs and find the D-Bus path for a given name.
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
        self.handle_list_configs().await?; // Refresh list
        Ok(())
    }

    async fn handle_start_live_vpn_logs(&self) -> Result<()> {
        // Ensure any previous log process is stopped
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

        // Store the process and drop the guard immediately
        {
            let mut guard = self.live_log_process.lock().unwrap();
            *guard = Some(cmd_process);
        } // Mutex guard is dropped here

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
        // Extract the child process and drop the mutex guard immediately
        let child_opt = {
            let mut guard = self.live_log_process.lock().unwrap();
            guard.take()
        }; // Mutex guard is dropped here

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
        cmd.arg("config-dump");
        if config_identifier.starts_with("/net/openvpn/v3/configuration/") {
            cmd.args(["--path", config_identifier]);
        } else {
            cmd.args(["--name", config_identifier]);
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
        // Try to find the name if identifier was a path, or use identifier if it was a name
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

        // Use session-manage with --password-prompt or stdin approach
        // This is more likely to be the correct OpenVPN3 command pattern
        let mut child = TokioCommand::new("openvpn3")
            .args(&[
                "session-manage",
                "--session-path",
                session_path,
                "--password-prompt",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Write the response to stdin
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

    // --- Parsing Helpers --- (These are simplified and might need more robust error handling)
    fn parse_sessions_list(&self, output: &str) -> Vec<SessionInfo> {
        // This parser assumes a specific block structure for each session.
        // It should be made more robust, e.g., by looking for "Session path:" to start a new session.
        let mut sessions = Vec::new();
        let mut current_session: Option<SessionInfo> = None;

        for line in output.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if line.starts_with("Session path:") {
                if let Some(session) = current_session.take() {
                    sessions.push(session);
                }
                current_session = Some(SessionInfo {
                    session_path: self.extract_value(line, "Session path:"),
                    config_name: String::new(),
                    status: String::new(),
                    created: String::new(),
                    owner: String::new(),
                });
            } else if let Some(session) = current_session.as_mut() {
                if line.starts_with("Config name:") {
                    session.config_name = self.extract_value(line, "Config name:");
                } else if line.starts_with("Status:") {
                    session.status = self.extract_value(line, "Status:");
                } else if line.starts_with("Created:") {
                    session.created = self.extract_value(line, "Created:");
                } else if line.starts_with("Owner:") {
                    session.owner = self.extract_value(line, "Owner:");
                }
            }
        }
        if let Some(session) = current_session.take() {
            // Add the last session
            sessions.push(session);
        }
        sessions
    }

    fn parse_configs_list(&self, output: &str) -> Vec<ConfigProfile> {
        let mut configs = Vec::new();
        let mut current_config: Option<ConfigProfile> = None;

        for line in output.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if line.starts_with("Configuration path:") {
                if let Some(config) = current_config.take() {
                    configs.push(config);
                }
                current_config = Some(ConfigProfile {
                    path: self.extract_value(line, "Configuration path:"),
                    name: String::new(),
                    import_time: String::new(),
                    owner: String::new(),
                    valid: true, // Assume valid if listed
                });
            } else if let Some(config) = current_config.as_mut() {
                if line.starts_with("Configuration name:") {
                    config.name = self.extract_value(line, "Configuration name:");
                } else if line.starts_with("Imported:") {
                    config.import_time = self.extract_value(line, "Imported:");
                } else if line.starts_with("Owner:") {
                    config.owner = self.extract_value(line, "Owner:");
                } else if line.starts_with("Last Used:") { // Example of another field
                     // config.last_used = self.extract_value(line, "Last Used:");
                }
            }
        }
        if let Some(config) = current_config.take() {
            // Add the last config
            configs.push(config);
        }
        configs
    }

    fn extract_value(&self, line: &str, key: &str) -> String {
        line.split_once(key)
            .map(|(_, val)| val.trim().to_string())
            .unwrap_or_default()
    }

    fn parse_session_stats(&self, output: &str) -> Option<SessionStats> {
        // This parser is very basic. A more robust solution would use regex or more detailed line checking.
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
            if let Some(val_str) = self.extract_field(line, "Bytes received:") {
                stats.bytes_in = val_str.parse().unwrap_or(0);
                found_any = true;
            } else if let Some(val_str) = self.extract_field(line, "Bytes sent:") {
                stats.bytes_out = val_str.parse().unwrap_or(0);
                found_any = true;
            } else if let Some(val_str) = self.extract_field(line, "Packets received:") {
                stats.packets_in = val_str.parse().unwrap_or(0);
                found_any = true;
            } else if let Some(val_str) = self.extract_field(line, "Packets sent:") {
                stats.packets_out = val_str.parse().unwrap_or(0);
                found_any = true;
            } else if let Some(val_str) = self.extract_field(line, "Connected since:") {
                stats.connected_since = val_str;
                found_any = true;
            } else if let Some(val_str) = self.extract_field(line, "Virtual IP:") {
                stats.virtual_ip = val_str;
                found_any = true;
            } else if let Some(val_str) = self.extract_field(line, "Remote IP:") {
                stats.remote_ip = val_str;
                found_any = true;
            }
        }
        if found_any {
            Some(stats)
        } else {
            None
        }
    }

    fn extract_field(&self, text: &str, field_key: &str) -> Option<String> {
        text.lines()
            .find(|line| line.trim().starts_with(field_key))
            .and_then(|line| line.split(':').nth(1))
            .map(|s| s.trim().to_string())
    }

    // --- Internal State Management & Messaging ---
    fn set_status(&self, new_status: ConnectionStatus) {
        let mut status_guard = self.status.lock().unwrap();
        *status_guard = new_status.clone();
        // Drop the guard before sending message to avoid potential deadlock if UI thread calls back immediately
        drop(status_guard);
        self.send_message_to_ui(VpnMessage::StatusUpdate(new_status));
    }

    fn set_status_and_log_error(&self, error_msg: String, context: &str) {
        self.log_manager_message(format!("Error ({}): {}", context, error_msg));
        self.set_status(ConnectionStatus::Error(error_msg)); // This sends StatusUpdate
    }

    fn log_manager_message(&self, message: String) {
        if *self.manager_logging_active.lock().unwrap() {
            self.send_message_to_ui(VpnMessage::LogMessage(format!("[MANAGER] {}", message)));
        }
    }

    fn log_manager_error(&self, error_msg: String, context: &str) {
        // Manager errors are always sent to UI log, regardless of manager_logging_active
        self.send_message_to_ui(VpnMessage::LogMessage(format!(
            "[MANAGER ERROR] {}: {}",
            context, error_msg
        )));
        // We don't set global status to Error here unless it's a connection-fatal error.
        // The specific handler should decide if global status changes.
    }

    fn send_message_to_ui(&self, message: VpnMessage) {
        if let Err(e) = self.message_tx.send(message) {
            // If UI channel is broken, manager can't do much. Log to its own console.
            eprintln!("OpenVPN3Manager: Failed to send message to UI: {}", e);
        }
    }

    fn status_changed(&self, old: &ConnectionStatus, new: &ConnectionStatus) -> bool {
        // Compare relevant fields, not just discriminant, for more accurate change detection
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
            _ => std::mem::discriminant(old) != std::mem::discriminant(new), // Fallback to discriminant for other types
        }
    }
}
