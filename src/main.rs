// src/main.rs
use eframe::egui;
use std::sync::mpsc;
use tokio::runtime::Runtime;

mod app;
mod config;
mod openvpn;

use app::OpenVPN3App;

fn main() -> Result<(), eframe::Error> {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([600.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "OpenVPN3 Client",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(OpenVPN3App::new(cc)))
        }),
    )
}
