// src/app.rs (Cleaned up version)
use eframe::egui;
use std::sync::mpsc;
use tokio::runtime::Runtime;
use crate::config::{AppConfig, VpnConfig};
use crate::openvpn::{OpenVPN3Manager, VpnMessage, VpnCommand, ConnectionStatus, SessionInfo, SessionStats, ConfigProfile};

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
    selected_config: Option<usize>,
    show_import_config_dialog: bool,
    import_config_path: String,
    import_config_name: String,
    connection_status: ConnectionStatus,
    log_messages: Vec<String>,
    manager_started: bool,

    // Data
    sessions_list: Vec<SessionInfo>,
    configs_list: Vec<ConfigProfile>,
    session_stats: Option<SessionStats>,
    real_time_logging: bool,
}

impl OpenVPN3App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load().unwrap_or_default();
        let (message_tx, message_rx) = mpsc::channel();
        let runtime = Runtime::new().unwrap();

        Self {
            config,
            vpn_manager: None,
            runtime,
            message_rx,
            message_tx,
            active_tab: ActiveTab::Connection,
            selected_config: None,
            show_import_config_dialog: false,
            import_config_path: String::new(),
            import_config_name: String::new(),
            connection_status: ConnectionStatus::Disconnected,
            log_messages: Vec::new(),
            manager_started: false,
            sessions_list: Vec::new(),
            configs_list: Vec::new(),
            session_stats: None,
            real_time_logging: false,
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

            // Load initial data
            if let Some(ref manager) = self.vpn_manager {
                let _ = manager.list_configs();
                let _ = manager.list_sessions();
            }
        }
    }

    fn process_messages(&mut self) {
        while let Ok(message) = self.message_rx.try_recv() {
            match message {
                VpnMessage::StatusUpdate(status) => {
                    self.connection_status = status;
                }
                VpnMessage::LogMessage(msg) => {
                    self.log_messages.push(format!("[{}] {}",
                                                   chrono::Local::now().format("%H:%M:%S"), msg));
                    if self.log_messages.len() > 1000 {
                        self.log_messages.remove(0);
                    }
                }
                VpnMessage::Error(err) => {
                    self.log_messages.push(format!("[{}] Error: {}",
                                                   chrono::Local::now().format("%H:%M:%S"), err));
                }
                VpnMessage::SessionsList(sessions) => {
                    self.sessions_list = sessions;
                }
                VpnMessage::SessionStats(stats) => {
                    self.session_stats = Some(stats);
                }
                VpnMessage::ConfigsList(configs) => {
                    self.configs_list = configs;
                }
                VpnMessage::ConfigImported(msg) => {
                    self.log_messages.push(format!("[{}] Config imported: {}",
                                                   chrono::Local::now().format("%H:%M:%S"), msg));
                }
            }
        }
    }

    fn draw_tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.active_tab, ActiveTab::Connection, "Connection");
            ui.selectable_value(&mut self.active_tab, ActiveTab::Sessions, "Sessions");
            ui.selectable_value(&mut self.active_tab, ActiveTab::Configurations, "Configurations");
            ui.selectable_value(&mut self.active_tab, ActiveTab::Statistics, "Statistics");
            ui.selectable_value(&mut self.active_tab, ActiveTab::Logs, "Logs");
            ui.selectable_value(&mut self.active_tab, ActiveTab::Settings, "Settings");
        });
        ui.separator();
    }

    fn draw_connection_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Connection Management");

        // Connection status
        ui.horizontal(|ui| {
            ui.label("Status:");
            let (text, color) = match &self.connection_status {
                ConnectionStatus::Disconnected => ("Disconnected", egui::Color32::RED),
                ConnectionStatus::Connecting => ("Connecting...", egui::Color32::YELLOW),
                ConnectionStatus::Connected { ip, duration } => {
                    ui.label(format!("IP: {}, Duration: {}", ip, duration));
                    ("Connected", egui::Color32::GREEN)
                }
                ConnectionStatus::Disconnecting => ("Disconnecting...", egui::Color32::YELLOW),
                ConnectionStatus::Error(err) => {
                    ui.label(format!("Error: {}", err));
                    ("Error", egui::Color32::RED)
                }
            };
            ui.colored_label(color, text);
        });

        ui.separator();

        // Configuration selection from imported configs
        ui.horizontal(|ui| {
            ui.label("Configuration:");
            egui::ComboBox::from_label("")
                .selected_text(
                    self.selected_config
                        .and_then(|i| self.configs_list.get(i))
                        .map(|c| c.name.as_str())
                        .unwrap_or("Select configuration")
                )
                .show_ui(ui, |ui| {
                    for (i, config) in self.configs_list.iter().enumerate() {
                        ui.selectable_value(&mut self.selected_config, Some(i), &config.name);
                    }
                });
        });

        // Connection controls
        ui.horizontal(|ui| {
            let can_connect = matches!(self.connection_status, ConnectionStatus::Disconnected)
                && self.selected_config.is_some();
            let can_disconnect = matches!(
                self.connection_status, 
                ConnectionStatus::Connected { .. } | ConnectionStatus::Connecting
            );

            if ui.add_enabled(can_connect, egui::Button::new("Connect")).clicked() {
                if let Some(config_idx) = self.selected_config {
                    if let Some(config) = self.configs_list.clone().get(config_idx) {
                        self.connect_vpn(&config.path);
                    }
                }
            }

            if ui.add_enabled(can_disconnect, egui::Button::new("Disconnect")).clicked() {
                self.disconnect_vpn();
            }

            if ui.button("Refresh Status").clicked() {
                if let Some(ref manager) = self.vpn_manager {
                    let _ = manager.get_status_update();
                }
            }
        });
    }

    fn draw_sessions_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Active Sessions");

        ui.horizontal(|ui| {
            if ui.button("Refresh Sessions").clicked() {
                if let Some(ref manager) = self.vpn_manager {
                    let _ = manager.list_sessions();
                }
            }
        });

        ui.separator();

        if self.sessions_list.is_empty() {
            ui.label("No active sessions");
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for session in &self.sessions_list {
                    ui.group(|ui| {
                        ui.label(format!("Config: {}", session.config_name));
                        ui.label(format!("Status: {}", session.status));
                        ui.label(format!("Created: {}", session.created));
                        ui.label(format!("Owner: {}", session.owner));
                        ui.label(format!("Path: {}", session.session_path));
                    });
                }
            });
        }
    }

    fn draw_configurations_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Configuration Management");

        ui.horizontal(|ui| {
            if ui.button("Import Config").clicked() {
                self.show_import_config_dialog = true;
            }

            if ui.button("Refresh Configs").clicked() {
                if let Some(ref manager) = self.vpn_manager {
                    let _ = manager.list_configs();
                }
            }
        });

        ui.separator();

        if self.configs_list.is_empty() {
            ui.label("No configurations imported");
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for config in &self.configs_list.clone() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(format!("Name: {}", config.name));
                                ui.label(format!("Imported: {}", config.import_time));
                                ui.label(format!("Owner: {}", config.owner));
                                ui.label(format!("Path: {}", config.path));
                            });

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Remove").clicked() {
                                    if let Some(ref manager) = self.vpn_manager {
                                        let _ = manager.remove_config(config.path.clone());
                                    }
                                }
                            });
                        });
                    });
                }
            });
        }
    }

    fn draw_statistics_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Connection Statistics");

        ui.horizontal(|ui| {
            if ui.button("Refresh Stats").clicked() {
                if let Some(ref manager) = self.vpn_manager {
                    let _ = manager.get_session_stats();
                }
            }
        });

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
                ui.label(format!("Bytes Received: {}", Self::format_bytes(stats.bytes_in)));
                ui.label(format!("Bytes Sent: {}", Self::format_bytes(stats.bytes_out)));
                ui.label(format!("Packets Received: {}", stats.packets_in));
                ui.label(format!("Packets Sent: {}", stats.packets_out));
            });
        } else {
            ui.label("No statistics available (not connected)");
        }
    }

    fn draw_logs_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Logs");

        ui.horizontal(|ui| {
            if ui.checkbox(&mut self.real_time_logging, "Real-time logging").changed() {
                if let Some(ref manager) = self.vpn_manager {
                    if self.real_time_logging {
                        let _ = manager.start_logging();
                    } else {
                        let _ = manager.stop_logging();
                    }
                }
            }

            if ui.button("Clear Logs").clicked() {
                self.log_messages.clear();
            }
        });

        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for message in &self.log_messages {
                    ui.label(message);
                }
            });
    }

    fn draw_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");

        ui.group(|ui| {
            ui.label("Application Settings:");
            ui.checkbox(&mut self.config.auto_start, "Auto start with system");
            ui.checkbox(&mut self.config.minimize_to_tray, "Minimize to tray");
        });

        ui.horizontal(|ui| {
            if ui.button("Save Settings").clicked() {
                if let Err(e) = self.config.save() {
                    self.log_messages.push(format!("Failed to save settings: {}", e));
                } else {
                    self.log_messages.push("Settings saved successfully".to_string());
                }
            }
        });
    }

    fn draw_import_config_dialog(&mut self, ctx: &egui::Context) {
        if self.show_import_config_dialog {
            egui::Window::new("Import Configuration")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Configuration Name:");
                    ui.text_edit_singleline(&mut self.import_config_name);

                    ui.label("Config File Path:");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.import_config_path);
                        if ui.button("Browse").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("OpenVPN Config", &["ovpn", "conf"])
                                .pick_file()
                            {
                                self.import_config_path = path.display().to_string();
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        if ui.button("Import").clicked() && !self.import_config_name.is_empty() && !self.import_config_path.is_empty() {
                            if let Some(ref manager) = self.vpn_manager {
                                let _ = manager.import_config(
                                    self.import_config_path.clone(),
                                    self.import_config_name.clone()
                                );
                            }

                            self.import_config_name.clear();
                            self.import_config_path.clear();
                            self.show_import_config_dialog = false;
                        }

                        if ui.button("Cancel").clicked() {
                            self.import_config_name.clear();
                            self.import_config_path.clear();
                            self.show_import_config_dialog = false;
                        }
                    });
                });
        }
    }

    fn connect_vpn(&mut self, config_path: &str) {
        self.ensure_manager_started();

        if let Some(manager) = &self.vpn_manager {
            if let Err(e) = manager.connect(config_path.to_string()) {
                self.log_messages.push(format!("Failed to send connect command: {}", e));
            }
        }
    }

    fn disconnect_vpn(&mut self) {
        if let Some(manager) = &self.vpn_manager {
            if let Err(e) = manager.disconnect() {
                self.log_messages.push(format!("Failed to send disconnect command: {}", e));
            }
        }
    }

    fn format_bytes(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        format!("{:.2} {}", size, UNITS[unit_index])
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

        self.draw_import_config_dialog(ctx);

        ctx.request_repaint_after(std::time::Duration::from_millis(1000));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Clean shutdown
        if let Some(ref manager) = self.vpn_manager {
            let _ = manager.shutdown();
        }
    }
}
