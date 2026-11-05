use std::sync::Arc;
use std::time::Duration;

use reqwest::cookie::{CookieStore, Jar};
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, REFERER};
use reqwest::redirect::Policy;
use serde_json::Value;
use url::Url;

use crate::error::{Error, Result};
use crate::session::Session;

pub const QR_LOGIN_TTL_MS: i64 = 180_000;
const CAS_CLIENT_ID: &str = "532";
const CAS_VERSION: &str = "1.2";
const QR_LOGIN_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct QrLoginConfig {
    pub token_url: String,
    pub poll_url: String,
    pub account_info_url: String,
    pub scan_url: String,
    pub user_agent: String,
    pub timeout_seconds: u64,
}

impl Default for QrLoginConfig {
    fn default() -> Self {
        Self {
            token_url: "https://uop.quark.cn/cas/ajax/getTokenForQrcodeLogin".to_string(),
            poll_url: "https://uop.quark.cn/cas/ajax/getServiceTicketByQrcodeToken".to_string(),
            account_info_url: "https://pan.quark.cn/account/info".to_string(),
            scan_url: "https://su.quark.cn/4_eMHBJ".to_string(),
            user_agent: QR_LOGIN_USER_AGENT.to_string(),
            timeout_seconds: 20,
        }
    }
}

#[derive(Debug)]
pub enum QrLoginPoll {
    Waiting,
    Expired,
    Success(Session),
}

#[derive(Clone)]
pub struct QrLogin {
    http: reqwest::Client,
    jar: Arc<Jar>,
    config: QrLoginConfig,
    qr_url: String,
}

impl QrLogin {
    pub async fn start(config: QrLoginConfig) -> Result<Self> {
        let jar = Arc::new(Jar::default());
        let http = reqwest::Client::builder()
            .user_agent(&config.user_agent)
            .cookie_provider(jar.clone())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(config.timeout_seconds))
            .redirect(Policy::limited(10))
            .build()?;

        let mut token_url = Url::parse(&config.token_url)?;
        token_url
            .query_pairs_mut()
            .append_pair("client_id", CAS_CLIENT_ID)
            .append_pair("v", CAS_VERSION)
            .append_pair("request_id", &uuid::Uuid::new_v4().to_string());
        let response = http
            .get(token_url)
            .header(ACCEPT, "application/json, text/plain, */*")
            .header(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9")
            .header(REFERER, "https://pan.quark.cn/")
            .send()
            .await?;
        let status = response.status();
        let body = response.bytes().await?;
        if !status.is_success() {
            return Err(Error::api(
                status.as_u16() as i64,
                0,
                format!("获取扫码登录二维码失败（HTTP {status}）"),
            ));
        }
        let value: Value = serde_json::from_slice(&body)?;
        if value.get("status").and_then(Value::as_i64) != Some(2_000_000) {
            return Err(Error::Other(format!(
                "获取扫码登录二维码失败: {}",
                api_message(&value)
            )));
        }
        let token = value
            .get("data")
            .and_then(|data| data.get("members"))
            .and_then(|members| members.get("token"))
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| Error::Other("扫码登录响应缺少 token".to_string()))?;

        let mut qr_url = Url::parse(&config.scan_url)?;
        qr_url
            .query_pairs_mut()
            .append_pair("token", token)
            .append_pair("client_id", CAS_CLIENT_ID)
            .append_pair("ssb", "weblogin")
            .append_pair("uc_param_str", "")
            .append_pair(
                "uc_biz_str",
                "S:custom|OPT:SAREA@0|OPT:IMMERSIVE@1|OPT:BACK_BTN_STYLE@0",
            );

        Ok(Self {
            http,
            jar,
            config,
            qr_url: qr_url.to_string(),
        })
    }

    pub fn qr_url(&self) -> &str {
        &self.qr_url
    }

    pub async fn poll(&self) -> Result<QrLoginPoll> {
        let mut url = Url::parse(&self.config.poll_url)?;
        {
            let token = Url::parse(&self.qr_url)?
                .query_pairs()
                .find(|(name, _)| name == "token")
                .map(|(_, value)| value.into_owned())
                .ok_or_else(|| Error::Other("扫码登录 URL 缺少 token".to_string()))?;
            url.query_pairs_mut()
                .append_pair("client_id", CAS_CLIENT_ID)
                .append_pair("v", CAS_VERSION)
                .append_pair("token", &token)
                .append_pair("request_id", &uuid::Uuid::new_v4().to_string());
        }

        let response = self
            .http
            .get(url)
            .header(ACCEPT, "application/json, text/plain, */*")
            .header(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9")
            .header(REFERER, "https://pan.quark.cn/")
            .send()
            .await?;
        let status = response.status();
        let body = response.bytes().await?;
        if !status.is_success() {
            return Err(Error::api(
                status.as_u16() as i64,
                0,
                format!("查询扫码状态失败（HTTP {status}）"),
            ));
        }
        let value: Value = serde_json::from_slice(&body)?;
        let code = value
            .get("status")
            .and_then(Value::as_i64)
            .or_else(|| {
                value
                    .get("data")
                    .and_then(|data| data.get("status"))
                    .and_then(Value::as_i64)
            })
            .unwrap_or_default();
        match code {
            2_000_000 => {
                let ticket = value
                    .get("data")
                    .and_then(|data| data.get("members"))
                    .and_then(|members| members.get("service_ticket"))
                    .and_then(Value::as_str)
                    .filter(|ticket| !ticket.is_empty())
                    .ok_or_else(|| Error::Other("扫码登录响应缺少 service ticket".to_string()))?;
                self.exchange_ticket(ticket).await
            }
            50_004_001 => Ok(QrLoginPoll::Waiting),
            50_004_002 => Ok(QrLoginPoll::Expired),
            _ => Err(Error::Other(format!(
                "查询扫码状态失败: {}",
                api_message(&value)
            ))),
        }
    }

    async fn exchange_ticket(&self, ticket: &str) -> Result<QrLoginPoll> {
        let mut url = Url::parse(&self.config.account_info_url)?;
        url.query_pairs_mut()
            .append_pair("st", ticket)
            .append_pair("lw", "scan");
        let response = self
            .http
            .get(url)
            .header(ACCEPT, "application/json, text/plain, */*")
            .header(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9")
            .header(REFERER, "https://pan.quark.cn/")
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            return Err(Error::api(
                status.as_u16() as i64,
                0,
                format!("使用扫码凭证登录失败（HTTP {status}）"),
            ));
        }
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
            if value.get("success").and_then(Value::as_bool) == Some(false) {
                let code = value
                    .get("code")
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                let message = value
                    .get("msg")
                    .or_else(|| value.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("扫码凭证无效或已过期");
                return Err(Error::api(status.as_u16() as i64, code, message));
            }
        }

        let session = self.session_from_jar()?;
        Ok(QrLoginPoll::Success(session))
    }

    fn session_from_jar(&self) -> Result<Session> {
        let account_url = Url::parse(&self.config.account_info_url)?;
        let is_quark_host = account_url
            .host_str()
            .is_some_and(|host| host.ends_with("quark.cn"));
        let mut cookie_header = Vec::new();
        let mut urls = vec![account_url];
        if is_quark_host {
            for host in [
                "pan.quark.cn",
                "drive.quark.cn",
                "drive-pc.quark.cn",
                "quark.cn",
                "uop.quark.cn",
            ] {
                if let Ok(url) = Url::parse(&format!("https://{host}/")) {
                    urls.push(url);
                }
            }
        }
        for url in &urls {
            if let Some(value) = self.jar.cookies(url) {
                if let Ok(text) = value.to_str() {
                    cookie_header.push(text.to_string());
                }
            }
        }
        if cookie_header.is_empty() {
            return Err(Error::InvalidCookie(
                "扫码登录未返回登录 Cookie，请刷新二维码重试".to_string(),
            ));
        }
        Session::from_cookie_header(&cookie_header.join("; "))
    }
}

fn api_message(value: &Value) -> String {
    value
        .get("message")
        .or_else(|| value.get("msg"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .unwrap_or("服务端返回异常状态")
        .to_string()
}
