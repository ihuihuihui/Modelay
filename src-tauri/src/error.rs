use thiserror::Error;

pub type Result<T> = std::result::Result<T, ModelayError>;

#[derive(Debug, Error)]
pub enum ModelayError {
    #[error("{0}")]
    Message(String),
    #[error("文件操作失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("配置格式无效：{0}")]
    Toml(#[from] toml_edit::TomlError),
    #[error("数据格式无效：{0}")]
    Json(#[from] serde_json::Error),
    #[error("任务索引操作失败：{0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("网络请求失败：{0}")]
    Network(#[from] reqwest::Error),
    #[error("系统密钥存储失败：{0}")]
    Secret(#[from] keyring::Error),
    #[error("窗口操作失败：{0}")]
    Tauri(#[from] tauri::Error),
}

impl From<&str> for ModelayError {
    fn from(value: &str) -> Self {
        Self::Message(value.to_owned())
    }
}

impl From<String> for ModelayError {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}

pub fn command_error(error: ModelayError) -> String {
    error.to_string()
}
