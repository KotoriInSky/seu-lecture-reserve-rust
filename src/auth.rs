use crate::{config::Credentials, storage};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::blocking::{Client, Response};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use rsa::{Pkcs1v15Encrypt, RsaPublicKey, pkcs8::DecodePublicKey};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};

pub const ENTRY: &str = "https://ehall.seu.edu.cn/gsapp/sys/yddjzxxtjappseu/*default/index.do";
const AUTH: &str = "https://auth.seu.edu.cn/auth/casback/";
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

#[derive(Debug)]
pub struct SessionExpired;
impl std::fmt::Display for SessionExpired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "登录会话已失效，请重新认证")
    }
}
impl std::error::Error for SessionExpired {}

fn school_url(url: &url::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && matches!(url.host_str(), Some("ehall.seu.edu.cn" | "auth.seu.edu.cn"))
}

pub fn service_from_login_url(url: &url::Url) -> Result<String> {
    ensure!(
        url.host_str() == Some("auth.seu.edu.cn"),
        "未进入学校统一认证页，可能被代理或网络门户重定向"
    );
    let query = url
        .fragment()
        .and_then(|s| s.split_once('?').map(|(_, q)| q))
        .or(url.query())
        .context("认证页缺少 service 参数")?;
    let service = url::form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == "service")
        .map(|(_, v)| v.into_owned())
        .context("认证页缺少 service 参数")?;
    let target = url::Url::parse(&service)?;
    ensure!(
        school_url(&target) && target.host_str() == Some("ehall.seu.edu.cn"),
        "认证 service 不属于学校办事大厅"
    );
    Ok(service)
}

pub fn encrypt_field(key: &str, text: &str) -> Result<String> {
    let normalized = key.replace('-', "+").replace('_', "/");
    let public = RsaPublicKey::from_public_key_der(
        &STANDARD.decode(normalized).context("认证公钥编码不正确")?,
    )
    .context("认证公钥格式不正确")?;
    Ok(STANDARD.encode(
        public
            .encrypt(&mut rand::thread_rng(), Pkcs1v15Encrypt, text.as_bytes())
            .context("无法加密认证字段")?,
    ))
}

pub fn checked_response(response: Response) -> Result<Response> {
    let status = response.status();
    if response.url().host_str() == Some("auth.seu.edu.cn") || status.as_u16() == 401 {
        return Err(SessionExpired.into());
    }
    ensure!(
        response.url().host_str() == Some("ehall.seu.edu.cn"),
        "登录会话已失效或请求进入网络门户，请重新认证"
    );
    ensure!(status.is_success(), "学校接口返回 HTTP {}", status.as_u16());
    Ok(response)
}

pub fn send(request: reqwest::blocking::RequestBuilder) -> Result<Response> {
    request
        .send()
        .map_err(|e| anyhow::anyhow!(e.without_url()))
        .context("请求学校服务器失败")
}

pub struct SchoolClient {
    pub http: Client,
    login_http: Client,
    jar: Arc<CookieStoreMutex>,
    session_path: Option<PathBuf>,
}

impl SchoolClient {
    pub fn clear_session(&self) -> Result<()> {
        self.jar
            .lock()
            .map_err(|_| anyhow::anyhow!("会话锁不可用"))?
            .clear();
        Ok(())
    }
    pub fn new(session_path: PathBuf, use_system_proxy: bool) -> Result<Self> {
        let session_path = storage::persistence_available().then_some(session_path);
        Self::with_session_path(session_path, use_system_proxy)
    }

    /// Keep cookies in memory only, including on Windows.
    pub fn new_ephemeral(use_system_proxy: bool) -> Result<Self> {
        Self::with_session_path(None, use_system_proxy)
    }

    fn with_session_path(session_path: Option<PathBuf>, use_system_proxy: bool) -> Result<Self> {
        let bytes = session_path.as_ref().map(std::fs::read).transpose();
        let store = match bytes {
            Ok(Some(bytes)) => {
                let plain = storage::open(&bytes)?;
                cookie_store::serde::json::load(plain.as_slice())
                    .map_err(|_| anyhow::anyhow!("保存的会话格式无效，请重新登录"))?
            }
            Ok(None) => CookieStore::default(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => CookieStore::default(),
            Err(e) => return Err(e).context("读取会话失败"),
        };
        let jar = Arc::new(CookieStoreMutex::new(store));
        let mut builder = Client::builder()
            .user_agent(USER_AGENT)
            .cookie_provider(jar.clone())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(25))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 {
                    attempt.error("重定向次数过多")
                } else if school_url(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }));
        if !use_system_proxy {
            builder = builder.no_proxy();
        }
        let mut login_builder = Client::builder()
            .user_agent(USER_AGENT)
            .cookie_provider(jar.clone())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(25))
            .redirect(reqwest::redirect::Policy::none());
        if !use_system_proxy {
            login_builder = login_builder.no_proxy();
        }
        Ok(Self {
            http: builder.build()?,
            login_http: login_builder.build()?,
            jar,
            session_path,
        })
    }

    pub fn save_session(&self) -> Result<()> {
        let Some(session_path) = &self.session_path else {
            return Ok(());
        };
        let guard = self
            .jar
            .lock()
            .map_err(|_| anyhow::anyhow!("会话锁不可用"))?;
        let mut plain = Vec::new();
        cookie_store::serde::json::save_incl_expired_and_nonpersistent(&guard, &mut plain)
            .map_err(|_| anyhow::anyhow!("会话序列化失败"))?;
        if let Some(parent) = session_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(session_path, storage::seal(&plain)?)?;
        Ok(())
    }

    fn auth_json(&self, endpoint: &str, body: &Value) -> Result<Value> {
        let response = send(self.http.post(format!("{AUTH}{endpoint}")).json(body))?;
        ensure!(
            response.status().is_success() && response.url().host_str() == Some("auth.seu.edu.cn"),
            "统一认证接口不可用"
        );
        response
            .json()
            .map_err(|e| anyhow::anyhow!(e.without_url()))
            .context("统一认证返回了无法识别的数据")
    }

    pub fn login(
        &self,
        credentials: &Credentials,
        mut read_sms: impl FnMut() -> Result<String>,
    ) -> Result<()> {
        let response = send(self.login_http.get(ENTRY))?;
        let service = if response.url().host_str() == Some("ehall.seu.edu.cn")
            && response.status().is_success()
        {
            // Authentication still needs verification through the business API by caller.
            self.save_session()?;
            return Ok(());
        } else {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .context("学校登录入口没有返回认证重定向")?
                .to_str()?;
            service_from_login_url(&response.url().join(location)?)?
        };
        let mut sms = String::new();
        for attempt in 0..2 {
            let key = self.auth_json("getChiperKey", &json!({}))?;
            let key = key
                .get("publicKey")
                .and_then(Value::as_str)
                .context("认证服务未返回公钥")?;
            let response=self.auth_json("casLogin",&json!({
                "service":service,"username":credentials.card_number,"password":encrypt_field(key,&credentials.password)?,
                "captcha":"","rememberMe":false,"loginType":"account","wxBinded":false,
                "mobilePhoneNum":"","mobileVerifyCode":if sms.is_empty(){String::new()}else{encrypt_field(key,&sms)?},"fingerPrint":null
            }))?;
            match response.get("code").and_then(Value::as_i64) {
                Some(200) => {
                    let encoded = response
                        .get("redirectUrl")
                        .and_then(Value::as_str)
                        .context("认证成功但缺少回跳地址")?;
                    // The school already percent-encodes redirectUrl; encoding it again breaks CAS.
                    let decoded = url::form_urlencoded::parse(format!("u={encoded}").as_bytes())
                        .next()
                        .context("回跳地址为空")?
                        .1
                        .into_owned();
                    ensure!(
                        school_url(&url::Url::parse(&decoded)?),
                        "认证回跳地址不属于学校"
                    );
                    checked_response(send(
                        self.http
                            .get(format!("{AUTH}loginRedirect?redirectUrl={encoded}")),
                    )?)?;
                    self.save_session()?;
                    return Ok(());
                }
                Some(502) if attempt == 0 => {
                    let sent = self
                        .auth_json("sendStage2Code", &json!({"userId":credentials.card_number}))?;
                    ensure!(
                        sent.get("code").and_then(Value::as_i64) == Some(200),
                        "短信验证码发送失败，请稍后重试"
                    );
                    sms = read_sms()?;
                    ensure!(!sms.trim().is_empty(), "短信验证码为空，已停止登录");
                    sms = sms.trim().to_owned();
                }
                Some(4000) => bail!("学校要求登录图形验证码；当前尚未验证该认证分支，请稍后再试"),
                code => bail!(
                    "统一认证未通过（状态 {:?}），请检查账号、密码或短信验证码",
                    code
                ),
            }
        }
        bail!("统一认证未完成")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{
        Engine,
        engine::general_purpose::{STANDARD, URL_SAFE},
    };
    use rsa::{Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey, pkcs8::EncodePublicKey};

    #[test]
    fn ephemeral_session_keeps_cookies_in_memory() {
        let client = SchoolClient::new_ephemeral(false).unwrap();
        let url = url::Url::parse(ENTRY).unwrap();
        client
            .jar
            .lock()
            .unwrap()
            .parse("test_session=memory_only; Secure; Path=/", &url)
            .unwrap();
        client.save_session().unwrap();
        assert!(client.session_path.is_none());
        assert_eq!(
            client.jar.lock().unwrap().get_request_values(&url).count(),
            1
        );
    }

    #[test]
    fn platform_session_policy_preserves_existing_files() {
        let dir = std::env::temp_dir().join(format!("seu-session-{}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("session.dpapi");
        let original = b"unreadable-session-from-another-platform";
        std::fs::write(&path, original).unwrap();
        let result = SchoolClient::new(path.clone(), false);
        if storage::persistence_available() {
            assert!(result.is_err());
        } else {
            let client = result.unwrap();
            client.save_session().unwrap();
            assert!(client.session_path.is_none());
        }
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn reads_fragment_service_and_rejects_foreign_redirect() {
        let u=url::Url::parse("https://auth.seu.edu.cn/dist/#/dist/main/login?service=http%3A%2F%2Fehall.seu.edu.cn%2Fgsapp%2Findex.do").unwrap();
        assert_eq!(
            service_from_login_url(&u).unwrap(),
            "http://ehall.seu.edu.cn/gsapp/index.do"
        );
        let u = url::Url::parse(
            "https://auth.seu.edu.cn/dist/#/main/login?service=https%3A%2F%2Fevil.example%2F",
        )
        .unwrap();
        assert!(service_from_login_url(&u).is_err());
    }
    #[test]
    fn accepts_urlsafe_school_rsa_key_and_preserves_leading_zero() {
        let private = RsaPrivateKey::new(&mut rand::thread_rng(), 1024).unwrap();
        let public = RsaPublicKey::from(&private).to_public_key_der().unwrap();
        let key = URL_SAFE.encode(public.as_bytes());
        let encrypted = encrypt_field(&key, "012345").unwrap();
        assert_eq!(
            private
                .decrypt(Pkcs1v15Encrypt, &STANDARD.decode(encrypted).unwrap())
                .unwrap(),
            b"012345"
        );
    }
}
