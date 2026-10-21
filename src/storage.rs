use anyhow::{Context, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::path::Path;

/// Other platforms keep credentials and cookies in memory until a secure
/// persistent backend is available. Never fall back to plaintext files.
pub const fn persistence_available() -> bool {
    cfg!(windows)
}

pub fn seal(plain: &[u8]) -> Result<Vec<u8>> {
    transform(plain, false)
}
pub fn open(encrypted: &[u8]) -> Result<Vec<u8>> {
    transform(encrypted, true)
}

#[cfg(windows)]
fn transform(input: &[u8], decrypt: bool) -> Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    let source = CRYPT_INTEGER_BLOB {
        cbData: input.len().try_into()?,
        pbData: input.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // DPAPI owns output until LocalFree; input remains alive for the entire call.
    unsafe {
        let ok = if decrypt {
            CryptUnprotectData(
                &source,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptProtectData(
                &source,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error())
                .context("Windows 无法加密或解密本地数据，请在原 Windows 用户下使用或重新登录");
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        Ok(bytes)
    }
}

#[cfg(not(windows))]
fn transform(_: &[u8], _: bool) -> Result<Vec<u8>> {
    anyhow::bail!("当前版本仅支持 Windows DPAPI 安全存储")
}

pub fn save_secret<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let encrypted = seal(&serde_json::to_vec(value)?)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, encrypted).context("无法保存加密数据")
}

pub fn load_secret<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(
            serde_json::from_slice(&open(&bytes)?).context("加密数据格式不正确")?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("无法读取加密数据"),
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn secrets_round_trip_and_corruption_is_rejected() {
        let secret = b"sample-session-cookie";
        let encrypted = seal(secret).unwrap();
        assert!(!encrypted.windows(secret.len()).any(|w| w == secret));
        assert_eq!(open(&encrypted).unwrap(), secret);
        let mut damaged = encrypted;
        let last = damaged.len() - 1;
        damaged[last] ^= 0xff;
        assert!(open(&damaged).is_err());
    }
}
