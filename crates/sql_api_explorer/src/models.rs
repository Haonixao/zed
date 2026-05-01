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
#[derive(Debug, Clone, Default)]
pub struct SqlApiHosts {
    pub hosts: Vec<HostConfig>,
}

impl gpui::Global for SqlApiHosts {}