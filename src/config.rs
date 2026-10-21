use crate::domain::Preferences;
use anyhow::{Context, Result, ensure};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub card_number: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OcrPaths {
    pub model: PathBuf,
    pub dictionary: PathBuf,
    pub runtime: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub preferences: Preferences,
    pub buffer_minutes: i64,
    pub use_system_proxy: bool,
    pub ocr: Option<OcrPaths>,
    pub last_selected_ids: Vec<String>,
}

pub fn data_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("cn", "SEU", "LectureReserve").context("无法确定本地配置目录")?;
    let path = dirs.data_local_dir().to_path_buf();
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = data_dir()?.join("preferences.toml");
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let config: Self = toml::from_str(&text).context("偏好配置格式不正确")?;
                ensure!(
                    (0..=1440).contains(&config.buffer_minutes),
                    "场次缓冲时间应在 0–1440 分钟之间"
                );
                Ok(config)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).context("读取偏好配置失败"),
        }
    }
    pub fn save(&self) -> Result<()> {
        std::fs::write(
            data_dir()?.join("preferences.toml"),
            toml::to_string_pretty(self)?,
        )?;
        Ok(())
    }
}
