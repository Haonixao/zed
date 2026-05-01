use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthConfig {
    pub r#type: String, // "none" | "bearer" | "custom-header" | "basic"
    pub token: Option<String>,
    pub header_name: Option<String>,
    pub header_value: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostConfig {
    pub name: String,
    pub auth: Option<AuthConfig>,
}

impl HostConfig {
    pub fn new(name: String) -> Self {
        Self {
            name,
            auth: Some(AuthConfig {
                r#type: "none".to_string(),
                token: None,
                header_name: None,
                header_value: None,
                username: None,
                password: None,
            }),
        }
    }

    pub fn auth_summary(&self) -> String {
        match &self.auth {
            Some(auth) => match auth.r#type.as_str() {
                "bearer" => "🔑 Bearer".to_string(),
                "custom-header" => "🔖 Custom Header".to_string(),
                "basic" => "🔐 Basic".to_string(),
                _ => "🌐 None".to_string(),
            },
            None => "🌐 None".to_string(),
        }
    }
}

// ==================== GLOBAL STATE ====================
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SqlApiHosts {
    pub hosts: Vec<HostConfig>,
}

impl SqlApiHosts {
    const STORAGE_FILENAME: &'static str = "sql_api_explorer_hosts.json";

    fn get_zed_data_dir() -> PathBuf {
        if let Some(data_dir) = std::env::var_os("ZED_DATA_DIR") {
            return PathBuf::from(data_dir);
        }
        if cfg!(target_os = "windows") {
            if let Some(app_data) = std::env::var_os("APPDATA") {
                return PathBuf::from(app_data).join("Zed");
            }
        } else if cfg!(target_os = "macos") {
            if let Some(home) = std::env::var_os("HOME") {
                return PathBuf::from(home).join("Library/Application Support/Zed");
            }
        }
        if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(data_home).join("zed");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share/zed");
        }
        PathBuf::from(".")
    }

    pub fn load() -> Result<Self> {
        let storage_path = Self::get_zed_data_dir().join(Self::STORAGE_FILENAME);
        if storage_path.exists() {
            let content = std::fs::read_to_string(&storage_path)
                .with_context(|| format!("Failed to read {}", storage_path.display()))?;
            let hosts: SqlApiHosts = serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse {}", storage_path.display()))?;
            Ok(hosts)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let data_dir = Self::get_zed_data_dir();
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create dir {}", data_dir.display()))?;
        let storage_path = data_dir.join(Self::STORAGE_FILENAME);
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize hosts")?;
        std::fs::write(&storage_path, content)
            .with_context(|| format!("Failed to write {}", storage_path.display()))?;
        Ok(())
    }
}

impl gpui::Global for SqlApiHosts {}