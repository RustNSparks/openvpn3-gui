// src/app.rs (Fixed version with proper configuration handling)
use crate::config::AppConfig;
use crate::openvpn::{
    ConfigProfile, ConnectionStatus, OpenVPN3Manager, SessionInfo, SessionStats, VpnMessage,
};
use eframe::egui;
use std::sync::mpsc;
use tokio::runtime::Runtime;

#[derive(PartialEq)]
enum ActiveTab {
    Connection,
    Sessions,
    Configurations,
    Statistics,
    Logs,
    Settings,
}

pub struct OpenVPN3App {
    config: AppConfig,
    vpn_manager: Option<OpenVPN3Manager>,
    runtime: Runtime,
    message_rx: mpsc::Receiver<VpnMessage>,
    message_tx: mpsc::Sender<VpnMessage>,

    // UI State
    active_tab: ActiveTab,
    selected_config_idx: Option<usize>,

    // Import Config Dialog
    show_import_config_dialog: bool,
    import_config_path: String,
    import_config_name: String,

    // Config Dump Dialog
    show_config_dump_dialog: bool,
    dumped_config_name: String,
    dumped_config_content: String,

    // Authentication Dialog
    show_auth_dialog: bool,
    auth_session_path: Option<String>,
    auth_prompt_message: String,
    auth_input_response: String,

    // Error Dialog
    show_error_dialog: bool,
    current_error_message: String,

    connection_status: ConnectionStatus,
    log_messages: Vec<String>,
    manager_started: bool,

    // Data from VPN Manager
    sessions_list: Vec<SessionInfo>,
    configs_list: Vec<ConfigProfile>,
    session_stats: Option<SessionStats>,
    vpn_cli_version: Option<String>,

    // Logging toggles
    live_vpn_logs_active: bool,
    manager_log_buffer_active: bool,

    // Recent errors for display
    recent_errors: Vec<String>,
}

impl OpenVPN3App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load().unwrap_or_default();
        let (message_tx, message_rx) = mpsc::channel();
        let runtime = Runtime::new().expect("Failed to create Tokio runtime");

        Self {
            config,
            vpn_manager: None,
            runtime,
            message_rx,
            message_tx,
            active_tab: ActiveTab::Connection,
            selected_config_idx: None,

            show_import_config_dialog: false,
            import_config_path: String::new(),
            import_config_name: String::new(),

            show_config_dump_dialog: false,
            dumped_config_name: String::new(),
            dumped_config_content: String::new(),

            show_auth_dialog: false,
            auth_session_path: None,
            auth_prompt_message: String::new(),
            auth_input_response: String::new(),

            show_error_dialog: false,
            current_error_message: String::new(),

            connection_status: ConnectionStatus::Disconnected,
            log_messages: Vec::new(),
            manager_started: false,

            sessions_list: Vec::new(),
            configs_list: Vec::new(),
            session_stats: None,
            vpn_cli_version: None,

            live_vpn_logs_active: false,
            manager_log_buffer_active: true,

            recent_errors: Vec::new(),
        }
    }

    fn ensure_manager_started(&mut self) {
        if !self.manager_started {
            let (manager, command_rx) = OpenVPN3Manager::new(self.message_tx.clone());
            let manager_clone = manager.clone();

            self.runtime.spawn(async move {
                manager_clone.run_manager_loop(command_rx).await;
            });

            self.vpn_manager = Some(manager);
            self.manager_started = true;
            self.add_log_entry("VPN Manager started.".to_string());

            // Load initial data & set initial log states
            if let Some(ref manager) = self.vpn_manager {
                let _ = manager.list_configs();
                let _ = manager.list_sessions();
                let _ = manager.get_vpn_cli_version();

                if self.live_vpn_logs_active {
                    let _ = manager.start_live_vpn_logs();
                }
                if self.manager_log_buffer_active {
                    let _ = manager.enable_manager_log_buffer();
                } else {
                    let _ = manager.disable_manager_log_buffer();
                }
            }
        }
    }

    fn add_log_entry(&mut self, message: String) {
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
        self.log_messages
            .push(format!("[{}] {}", timestamp, message));
        if self.log_messages.len() > 1000 {
            self.log_messages.remove(0);
        }
    }

    fn add_error_entry(&mut self, error_message: String) {
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
        let formatted_error = format!("[{}] 🔴 CRITICAL ERROR: {}", timestamp, error_message);

        // Add to logs
        self.log_messages.push(formatted_error.clone());
        if self.log_messages.len() > 1000 {
            self.log_messages.remove(0);
        }

        // Add to recent errors
        self.recent_errors.push(formatted_error);
        if self.recent_errors.len() > 10 {
            self.recent_errors.remove(0);
        }

        // Show error dialog for critical errors
        if error_message.contains("Connection failed")
            || error_message.contains("Authentication failed")
            || error_message.contains("Failed to load initial")
        {
            self.current_error_message = error_message;
            self.show_error_dialog = true;
        }
    }

    fn process_messages(&mut self) {
        while let Ok(message) = self.message_rx.try_recv() {
            match message {
                VpnMessage::StatusUpdate(status) => {
                    self.connection_status = status.clone();
                    if let ConnectionStatus::AuthenticationRequired {
                        session_path,
                        prompt,
                    } = status
                    {
                        self.auth_session_path = Some(session_path);
                        self.auth_prompt_message = prompt;
                        self.auth_input_response.clear();
                        self.show_auth_dialog = true;
                    }
                }
                VpnMessage::LogMessage(msg) => {
                    self.log_messages.push(msg);
                    if self.log_messages.len() > 1000 {
                        self.log_messages.remove(0);
                    }
                }
                VpnMessage::Error(err_msg) => {
                    // Handle critical errors with special treatment
                    self.add_error_entry(err_msg);
                }
                VpnMessage::SessionsList(sessions) => {
                    self.sessions_list = sessions;
                }
                VpnMessage::SessionStats(stats) => {
                    self.session_stats = Some(stats);
                }
                VpnMessage::ConfigsList(configs) => {
                    self.configs_list = configs;
                    // Auto-select the first config if none selected and configs exist
                    if self.selected_config_idx.is_none() && !self.configs_list.is_empty() {
                        self.selected_config_idx = Some(0);
                    }
                    self.add_log_entry(format!(
                        "Loaded {} configurations",
                        self.configs_list.len()
                    ));
                }
                VpnMessage::ConfigImported(msg) => {
                    self.add_log_entry(format!("Config imported: {}", msg));
                    // Request refresh of config list after import
                    if let Some(ref manager) = self.vpn_manager {
                        let _ = manager.list_configs();
                    }
                }
                VpnMessage::VpnCliVersion(version) => {
                    self.vpn_cli_version = Some(version);
                }
                VpnMessage::ConfigDumped { name, content } => {
                    self.dumped_config_name = name;
                    self.dumped_config_content = content;
                    self.show_config_dump_dialog = true;
                }
                VpnMessage::AuthenticationPrompt {
                    session_path,
                    prompt,
                } => {
                    self.auth_session_path = Some(session_path.clone());
                    self.auth_prompt_message = prompt.clone();
                    self.auth_input_response.clear();
                    self.show_auth_dialog = true;
                    if !matches!(
                        self.connection_status,
                        ConnectionStatus::AuthenticationRequired { .. }
                    ) {
                        self.connection_status = ConnectionStatus::AuthenticationRequired {
                            session_path,
                            prompt,
                        };
                    }
                }
            }
        }
    }

    fn draw_tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.active_tab, ActiveTab::Connection, "Connection");
            ui.selectable_value(&mut self.active_tab, ActiveTab::Sessions, "Sessions");
            ui.selectable_value(
                &mut self.active_tab,
                ActiveTab::Configurations,
                "Configurations",
            );
            ui.selectable_value(&mut self.active_tab, ActiveTab::Statistics, "Statistics");
            ui.selectable_value(&mut self.active_tab, ActiveTab::Logs, "Logs");
            ui.selectable_value(&mut self.active_tab, ActiveTab::Settings, "Settings");
        });
        ui.separator();
    }

    fn draw_connection_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Connection Management");

        // Handle button clicks from error section
        let mut clear_errors = false;
        let mut switch_to_logs = false;

        // Show recent errors prominently in connection tab
        if !self.recent_errors.is_empty() {
            ui.group(|ui| {
                ui.colored_label(egui::Color32::RED, "⚠️ Recent Critical Errors:");

                for error in self.recent_errors.iter().rev().take(3) {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }

                if self.recent_errors.len() > 3 {
                    ui.colored_label(
                        egui::Color32::GRAY,
                        format!(
                            "... and {} more errors (check Logs tab)",
                            self.recent_errors.len() - 3
                        ),
                    );
                }

                ui.horizontal(|ui| {
                    if ui.button("Clear Errors").clicked() {
                        clear_errors = true;
                    }
                    if ui.button("View All in Logs").clicked() {
                        switch_to_logs = true;
                    }
                });
            });
            ui.separator();
        }

        // Handle button actions after the UI scope
        if clear_errors {
            self.recent_errors.clear();
        }
        if switch_to_logs {
            self.active_tab = ActiveTab::Logs;
        }

        // Connection status
        ui.horizontal(|ui| {
            ui.label("Status:");
            let (text_display, color) = match &self.connection_status {
                ConnectionStatus::Disconnected => ("Disconnected".to_string(), egui::Color32::RED),
                ConnectionStatus::Connecting => ("Connecting...".to_string(), egui::Color32::KHAKI),
                ConnectionStatus::Connected { ip, duration } => (
                    format!("Connected (IP: {}, Duration: {})", ip, duration),
                    egui::Color32::GREEN,
                ),
                ConnectionStatus::Disconnecting => {
                    ("Disconnecting...".to_string(), egui::Color32::KHAKI)
                }
                ConnectionStatus::Error(err) => (format!("Error: {}", err), egui::Color32::RED),
                ConnectionStatus::AuthenticationRequired { prompt, .. } => (
                    format!("Authentication Required: {}", prompt),
                    egui::Color32::LIGHT_BLUE,
                ),
            };
            ui.colored_label(color, text_display);
        });

        if matches!(
            self.connection_status,
            ConnectionStatus::AuthenticationRequired { .. }
        ) && ui.button("Enter Credentials").clicked()
        {
            self.show_auth_dialog = true;
        }

        ui.separator();

        // Configuration selection
        ui.horizontal(|ui| {
            ui.label("Configuration:");

            if self.configs_list.is_empty() {
                ui.label("No configurations available. Please import a configuration file.");
                if ui.button("Go to Configurations").clicked() {
                    self.active_tab = ActiveTab::Configurations;
                }
            } else {
                let selected_text = self
                    .selected_config_idx
                    .and_then(|idx| self.configs_list.get(idx))
                    .map_or("Select configuration", |c| &c.name);

                egui::ComboBox::from_id_salt("config_combobox")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        for (i, config_profile) in self.configs_list.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.selected_config_idx,
                                Some(i),
                                &config_profile.name,
                            );
                        }
                    });

                if ui
                    .button("🔄")
                    .on_hover_text("Refresh configuration list")
                    .clicked()
                    && let Some(ref manager) = self.vpn_manager
                {
                    let _ = manager.list_configs();
                }
            }
        });

        // Debug info - show selected config details
        if let Some(idx) = self.selected_config_idx
            && let Some(config) = self.configs_list.get(idx)
        {
            ui.label(format!("Selected: {} (Path: {})", config.name, config.path));
        }

        // Connection controls
        ui.horizontal(|ui| {
            let can_connect = (matches!(self.connection_status, ConnectionStatus::Disconnected)
                || matches!(self.connection_status, ConnectionStatus::Error(_)))
                && self.selected_config_idx.is_some();

            let can_disconnect = matches!(
                self.connection_status,
                ConnectionStatus::Connected { .. }
                    | ConnectionStatus::Connecting
                    | ConnectionStatus::AuthenticationRequired { .. }
            );

            if ui
                .add_enabled(can_connect, egui::Button::new("Connect"))
                .clicked()
                && let Some(idx) = self.selected_config_idx
                && let Some(config) = self.configs_list.get(idx)
            {
                // Use the config path if it starts with /net/openvpn, otherwise use the name
                let identifier = if config.path.starts_with("/net/openvpn/v3/configuration/") {
                    config.path.clone()
                } else {
                    config.name.clone()
                };
                self.connect_vpn(&identifier);
            }

            if ui
                .add_enabled(can_disconnect, egui::Button::new("Disconnect"))
                .clicked()
            {
                self.disconnect_vpn();
            }

            if ui.button("Refresh Status").clicked()
                && let Some(ref manager) = self.vpn_manager
            {
                let _ = manager.get_status_update();
            }
        });
    }

    fn draw_sessions_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Active Sessions");
        if ui.button("Refresh Sessions").clicked()
            && let Some(ref manager) = self.vpn_manager
        {
            let _ = manager.list_sessions();
        }
        ui.separator();

        if self.sessions_list.is_empty() {
            ui.label("No active OpenVPN3 sessions found by the manager.");
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for session in &self.sessions_list {
                    ui.group(|ui| {
                        ui.label(format!("Config Name: {}", session.config_name));
                        ui.label(format!("Session Path: {}", session.session_path));
                        ui.label(format!("Status: {}", session.status));
                        ui.label(format!("Created: {}", session.created));
                        ui.label(format!("Owner: {}", session.owner));
                    });
                }
            });
        }
    }

    fn draw_configurations_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Configuration Profiles");
        ui.horizontal(|ui| {
            if ui.button("Import Config File").clicked() {
                self.show_import_config_dialog = true;
            }
            if ui.button("Refresh List").clicked()
                && let Some(ref manager) = self.vpn_manager
            {
                let _ = manager.list_configs();
            }
        });
        ui.separator();

        if self.configs_list.is_empty() {
            ui.label(
                "No configurations found. Click 'Import Config File' to add your .ovpn files.",
            );
        } else {
            ui.label(format!(
                "Found {} configuration(s):",
                self.configs_list.len()
            ));

            let mut action_remove: Option<String> = None;
            let mut action_dump: Option<String> = None;

            let configs_clone = self.configs_list.clone();

            egui::ScrollArea::vertical().show(ui, |ui| {
                for config_profile in &configs_clone {
                    ui.group(|ui| {
                        ui.label(format!("Name: {}", config_profile.name));
                        ui.label(format!("Path: {}", config_profile.path));
                        ui.label(format!("Imported: {}", config_profile.import_time));
                        ui.label(format!("Owner: {}", config_profile.owner));
                        ui.label(format!("Valid: {}", config_profile.valid));
                        ui.horizontal(|ui| {
                            if ui
                                .button("Remove")
                                .on_hover_text("Remove this configuration")
                                .clicked()
                            {
                                action_remove = Some(config_profile.path.clone());
                            }
                            if ui
                                .button("View/Dump")
                                .on_hover_text("View raw configuration content")
                                .clicked()
                            {
                                action_dump = Some(config_profile.path.clone());
                            }
                        });
                    });
                }
            });

            if let Some(config_path_to_remove) = action_remove {
                let manager = self.vpn_manager.clone();
                if let Some(manager) = manager {
                    self.add_log_entry(format!(
                        "Requesting removal of config: {}",
                        config_path_to_remove
                    ));
                    let _ = manager.remove_config(config_path_to_remove);
                }
            }
            if let Some(config_path_to_dump) = action_dump {
                let manager = self.vpn_manager.clone();
                if let Some(manager) = manager {
                    self.add_log_entry(format!(
                        "Requesting dump of config: {}",
                        config_path_to_dump
                    ));
                    let _ = manager.dump_config(config_path_to_dump);
                }
            }
        }
    }

    fn draw_statistics_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Connection Statistics");
        if ui.button("Refresh Stats").clicked()
            && let Some(ref manager) = self.vpn_manager
        {
            let _ = manager.get_session_stats();
        }
        ui.separator();

        if let Some(ref stats) = self.session_stats {
            ui.group(|ui| {
                ui.label("Connection Information:");
                ui.label(format!("Virtual IP: {}", stats.virtual_ip));
                ui.label(format!("Remote IP: {}", stats.remote_ip));
                ui.label(format!("Connected Since: {}", stats.connected_since));
            });
            ui.group(|ui| {
                ui.label("Traffic Statistics:");
                ui.label(format!(
                    "Bytes Received: {}",
                    Self::format_bytes(stats.bytes_in)
                ));
                ui.label(format!(
                    "Bytes Sent: {}",
                    Self::format_bytes(stats.bytes_out)
                ));
                ui.label(format!("Packets Received: {}", stats.packets_in));
                ui.label(format!("Packets Sent: {}", stats.packets_out));
            });
        } else {
            ui.label("No statistics available (not connected or no active session).");
        }
    }

    fn draw_logs_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Application & VPN Logs");

        let manager_exists = self.vpn_manager.is_some();
        let mut live_logs_changed = false;
        let mut manager_logs_changed = false;

        ui.horizontal(|ui| {
            if ui
                .checkbox(
                    &mut self.live_vpn_logs_active,
                    "Stream Live VPN Logs (`openvpn3 log`)",
                )
                .changed()
            {
                live_logs_changed = true;
            }
            if ui
                .checkbox(
                    &mut self.manager_log_buffer_active,
                    "Enable Internal Manager Logs",
                )
                .changed()
            {
                manager_logs_changed = true;
            }
            if ui.button("Clear Displayed Logs").clicked() {
                self.log_messages.clear();
            }
            if ui.button("Clear Error History").clicked() {
                self.recent_errors.clear();
            }
        });

        // Handle checkbox changes after the UI section
        if manager_exists {
            if live_logs_changed && let Some(ref manager) = self.vpn_manager {
                if self.live_vpn_logs_active {
                    let _ = manager.start_live_vpn_logs();
                } else {
                    let _ = manager.stop_live_vpn_logs();
                }
            }
            if manager_logs_changed && let Some(ref manager) = self.vpn_manager {
                if self.manager_log_buffer_active {
                    let _ = manager.enable_manager_log_buffer();
                } else {
                    let _ = manager.disable_manager_log_buffer();
                }
            }
        }

        ui.separator();

        // Show error summary if there are recent errors
        if !self.recent_errors.is_empty() {
            ui.group(|ui| {
                ui.colored_label(
                    egui::Color32::RED,
                    format!(
                        "⚠️ {} Critical Errors in Session:",
                        self.recent_errors.len()
                    ),
                );
                egui::ScrollArea::vertical()
                    .max_height(100.0)
                    .show(ui, |ui| {
                        for error in &self.recent_errors {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                    });
            });
            ui.separator();
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for message in &self.log_messages {
                    if message.contains("🔴 CRITICAL ERROR") {
                        ui.colored_label(egui::Color32::RED, message);
                    } else if message.contains("[MANAGER ERROR]") {
                        ui.colored_label(egui::Color32::LIGHT_RED, message);
                    } else if message.contains("[MANAGER]") {
                        ui.colored_label(egui::Color32::LIGHT_BLUE, message);
                    } else {
                        ui.label(message);
                    }
                }
            });
    }

    fn draw_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Application Settings");
        ui.group(|ui| {
            ui.label("General Settings:");
            ui.checkbox(
                &mut self.config.auto_start,
                "Auto start with system (Not implemented)",
            );
            ui.checkbox(
                &mut self.config.minimize_to_tray,
                "Minimize to tray (Not implemented)",
            );
            if ui.button("Save App Settings").clicked() {
                if let Err(e) = self.config.save() {
                    self.add_log_entry(format!("Failed to save settings: {}", e));
                } else {
                    self.add_log_entry("Settings saved successfully.".to_string());
                }
            }
        });
        ui.group(|ui| {
            ui.label("OpenVPN3 Information:");
            if let Some(version) = &self.vpn_cli_version {
                ui.label(format!("CLI Version: {}", version));
            } else {
                ui.label("CLI Version: Unknown");
            }
            if ui.button("Refresh CLI Version").clicked()
                && let Some(ref manager) = self.vpn_manager
            {
                let _ = manager.get_vpn_cli_version();
            }
        });

        ui.separator();
        ui.group(|ui| {
            ui.label("Error Handling:");
            ui.label(format!(
                "Recent errors tracked: {}",
                self.recent_errors.len()
            ));
            if ui.button("Clear All Error History").clicked() {
                self.recent_errors.clear();
                self.add_log_entry("Error history cleared.".to_string());
            }
        });
    }

    fn draw_import_config_dialog(&mut self, ctx: &egui::Context) {
        if self.show_import_config_dialog {
            egui::Window::new("Import Configuration File")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("Configuration Name (for display):");
                    ui.text_edit_singleline(&mut self.import_config_name);
                    ui.add_space(5.0);
                    ui.label("Config File Path (.ovpn):");
                    ui.horizontal(|ui| {
                        let text_edit = egui::TextEdit::singleline(&mut self.import_config_path)
                            .desired_width(300.0);
                        ui.add(text_edit);
                        if ui.button("Browse...").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .add_filter("OpenVPN Config", &["ovpn", "conf"])
                                .pick_file()
                        {
                            self.import_config_path = path.display().to_string();
                            // Auto-fill name from filename if name is empty
                            if self.import_config_name.is_empty()
                                && let Some(filename) = path.file_stem()
                            {
                                self.import_config_name = filename.to_string_lossy().to_string();
                            }
                        }
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Import").clicked() {
                            if self.import_config_name.is_empty() {
                                self.add_error_entry(
                                    "Import failed: Configuration name cannot be empty."
                                        .to_string(),
                                );
                            } else if self.import_config_path.is_empty() {
                                self.add_error_entry(
                                    "Import failed: Configuration file path cannot be empty."
                                        .to_string(),
                                );
                            } else if let Some(ref manager) = self.vpn_manager {
                                // self.add_log_entry(format!(
                                //     "Importing config '{}' from '{}'",
                                //     self.import_config_name, self.import_config_path
                                // ));
                                println!(
                                    "Importing config '{}' from '{}'",
                                    self.import_config_name, self.import_config_path
                                );
                                let _ = manager.import_config(
                                    self.import_config_path.clone(),
                                    self.import_config_name.clone(),
                                );
                                self.show_import_config_dialog = false;
                                self.import_config_name.clear();
                                self.import_config_path.clear();
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_import_config_dialog = false;
                            self.import_config_name.clear();
                            self.import_config_path.clear();
                        }
                    });
                });
        }
    }

    fn draw_config_dump_dialog(&mut self, ctx: &egui::Context) {
        if self.show_config_dump_dialog {
            egui::Window::new(format!("Configuration: {}", self.dumped_config_name))
                .default_size([600.0, 400.0])
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(true)
                .resizable(true)
                .show(ctx, |ui| {
                    egui::ScrollArea::both().show(ui, |ui| {
                        ui.label(egui::RichText::new(&self.dumped_config_content).monospace());
                    });
                    ui.add_space(10.0);
                    if ui.button("Close").clicked() {
                        self.show_config_dump_dialog = false;
                        self.dumped_config_name.clear();
                        self.dumped_config_content.clear();
                    }
                });
        }
    }

    fn draw_authentication_dialog(&mut self, ctx: &egui::Context) {
        if self.show_auth_dialog {
            egui::Window::new("VPN Authentication Required")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("The VPN connection requires authentication.");
                    ui.add_space(5.0);
                    ui.label(egui::RichText::new(&self.auth_prompt_message).strong());
                    ui.add_space(5.0);
                    ui.label("Response:");

                    let is_password_prompt =
                        self.auth_prompt_message.to_lowercase().contains("password");
                    let text_edit_response = if is_password_prompt {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.auth_input_response)
                                .password(true),
                        )
                    } else {
                        ui.text_edit_singleline(&mut self.auth_input_response)
                    };

                    if text_edit_response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        let manager = self.vpn_manager.clone();
                        let session_path = self.auth_session_path.clone();
                        if let (Some(manager), Some(session_path)) = (manager, session_path) {
                            let _ = manager.submit_authentication(
                                session_path,
                                self.auth_input_response.clone(),
                            );
                        }
                        self.show_auth_dialog = false;
                    }
                    text_edit_response.request_focus();

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Submit").clicked() {
                            let manager = self.vpn_manager.clone();
                            let session_path = self.auth_session_path.clone();
                            if let (Some(manager), Some(session_path)) = (manager, session_path) {
                                let _ = manager.submit_authentication(
                                    session_path,
                                    self.auth_input_response.clone(),
                                );
                            }
                            self.show_auth_dialog = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_auth_dialog = false;
                            self.add_log_entry("Authentication cancelled by user.".to_string());
                            if matches!(
                                self.connection_status,
                                ConnectionStatus::AuthenticationRequired { .. }
                            ) {
                                self.disconnect_vpn();
                            }
                        }
                    });
                });
        }
    }

    fn draw_error_dialog(&mut self, ctx: &egui::Context) {
        if self.show_error_dialog {
            egui::Window::new("⚠️ Critical Error")
                .collapsible(false)
                .resizable(true)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .default_size([500.0, 200.0])
                .show(ctx, |ui| {
                    ui.colored_label(egui::Color32::RED, "A critical error has occurred:");
                    ui.add_space(10.0);

                    egui::ScrollArea::vertical()
                        .max_height(150.0)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(&self.current_error_message).strong());
                        });

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            self.show_error_dialog = false;
                            self.current_error_message.clear();
                        }
                        if ui.button("View Logs").clicked() {
                            self.show_error_dialog = false;
                            self.current_error_message.clear();
                            self.active_tab = ActiveTab::Logs;
                        }
                        if ui.button("Retry Connection").clicked() {
                            self.show_error_dialog = false;
                            self.current_error_message.clear();
                            if let Some(idx) = self.selected_config_idx
                                && let Some(config) = self.configs_list.get(idx)
                            {
                                let identifier =
                                    if config.path.starts_with("/net/openvpn/v3/configuration/") {
                                        config.path.clone()
                                    } else {
                                        config.name.clone()
                                    };
                                self.connect_vpn(&identifier);
                            }
                        }
                    });
                });
        }
    }

    fn connect_vpn(&mut self, config_identifier: &str) {
        self.ensure_manager_started();
        let manager = self.vpn_manager.clone();
        if let Some(manager) = manager {
            self.add_log_entry(format!(
                "Sending connect command for: {}",
                config_identifier
            ));
            if let Err(e) = manager.connect(config_identifier.to_string()) {
                self.add_error_entry(format!("Failed to send connect command: {}", e));
            }
        }
    }

    fn disconnect_vpn(&mut self) {
        let manager = self.vpn_manager.clone();
        if let Some(manager) = manager {
            self.add_log_entry("Sending disconnect command.".to_string());
            if let Err(e) = manager.disconnect() {
                self.add_error_entry(format!("Failed to send disconnect command: {}", e));
            }
        }
    }

    fn format_bytes(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
        if bytes == 0 {
            return "0 B".to_string();
        }
        let i = (bytes as f64).log2() / 10.0;
        let i = i.floor() as usize;
        let i = if i >= UNITS.len() { UNITS.len() - 1 } else { i };
        let size = bytes as f64 / (1024.0_f64.powi(i as i32));
        format!("{:.2} {}", size, UNITS.get(i).unwrap_or(&"B"))
    }
}

impl eframe::App for OpenVPN3App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_manager_started();
        self.process_messages();

        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_tab_bar(ui);

            match self.active_tab {
                ActiveTab::Connection => self.draw_connection_tab(ui),
                ActiveTab::Sessions => self.draw_sessions_tab(ui),
                ActiveTab::Configurations => self.draw_configurations_tab(ui),
                ActiveTab::Statistics => self.draw_statistics_tab(ui),
                ActiveTab::Logs => self.draw_logs_tab(ui),
                ActiveTab::Settings => self.draw_settings_tab(ui),
            }
        });

        // Draw all dialogs
        self.draw_import_config_dialog(ctx);
        self.draw_config_dump_dialog(ctx);
        self.draw_authentication_dialog(ctx);
        self.draw_error_dialog(ctx);

        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.add_log_entry("Application exiting. Shutting down VPN manager.".to_string());
        if let Some(ref manager) = self.vpn_manager
            && let Err(e) = manager.shutdown()
        {
            eprintln!("Failed to send shutdown command to manager: {}", e);
        }
        if let Err(e) = self.config.save() {
            eprintln!("Failed to save app config on exit: {}", e);
        }
    }
}
