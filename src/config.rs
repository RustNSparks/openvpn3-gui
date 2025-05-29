// src/config.rs
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnConfig {
    pub name: String,
    pub config_path: PathBuf,
    pub auto_connect: bool,
    pub save_credentials: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub configs: Vec<VpnConfig>,
    pub last_used_config: Option<String>,
    pub auto_start: bool,
    pub minimize_to_tray: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            configs: Vec::new(),
            last_used_config: None,
            auto_start: false,
            minimize_to_tray: true,
        }
    }
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot find config directory"))?
            .join("openvpn3-gui");
        
        std::fs::create_dir_all(&config_dir)?;
        
        let config_file = config_dir.join("config.json");
        
        if config_file.exists() {
            let content = std::fs::read_to_string(config_file)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }
    
    pub fn save(&self) -> anyhow::Result<()> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot find config directory"))?
            .join("openvpn3-gui");
        
        std::fs::create_dir_all(&config_dir)?;
        
        let config_file = config_dir.join("config.json");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(config_file, content)?;
        
        Ok(())
    }
}
