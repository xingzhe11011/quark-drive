use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use httpdate::fmt_http_date;
use md5::{Digest as Md5Digest, Md5};
use reqwest::header::{
    HeaderMap, HeaderValue, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, SET_COOKIE, USER_AGENT,
};
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use url::Url;

use crate::error::{Error, Result};
use crate::models::{
    DownloadUrl, FileEntry, ListPage, MemberInfo, RecyclePage, UploadPre, UploadPreData,
    UploadPreMetadata,
};
use crate::session::Session;

const REFERER_VALUE: &str = "https://pan.quark.cn";
const OSS_USER_AGENT: &str = "aliyun-sdk-js/6.6.1 Chrome 98.0.4758.80 on Windows 10 64-bit";
pub const DEFAULT_QUARK_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) quark-cloud-drive/2.5.20 Chrome/100.0.4896.160 Electron/18.3.5.4-b478491100 Safari/537.36 Channel/pckk_other_ch";

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub api_base: String,
    pub pc_api_base: String,
    pub account_base: String,
    pub user_agent: String,
    /// Test hook. When set, OSS part URLs point here instead of the real bucket host.
    pub oss_base_override: Option<String>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            api_base: "https://drive.quark.cn/1/clouddrive".to_string(),
            pc_api_base: "https://drive-pc.quark.cn/1/clouddrive".to_string(),
            account_base: "https://pan.quark.cn".to_string(),
            user_agent: DEFAULT_QUARK_USER_AGENT.to_string(),
            oss_base_override: None,
        }
    }
}

#[derive(Clone)]
pub struct QuarkClient {
    http: reqwest::Client,
    session: Arc<Mutex<Session>>,
    config: Arc<ClientConfig>,
}

impl QuarkClient {
    pub fn new(session: Session, config: ClientConfig) -> Result<Self> {
        if !session.is_authenticated() {
            return Err(Error::AuthRequired);
        }
        let http = reqwest::Client::builder()
            .user_agent(&config.user_agent)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .pool_max_idle_per_host(16)
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()?;
        Ok(Self {
            http,
            session: Arc::new(Mutex::new(session)),
            config: Arc::new(config),
        })
    }

    pub fn session(&self) -> Session {
        self.session.lock().expect("session mutex poisoned").clone()
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    pub async fn get_config(&self) -> Result<()> {
        let url = self.api_url("/config")?;
        let req = self.http.get(url);
        self.send_json(req).await.map(|_| ())
    }

    pub async fn member_info(&self) -> Result<MemberInfo> {
        let mut url = self.api_url("/member")?;
        url.query_pairs_mut()
            .append_pair("fetch_subscribe", "true")
            .append_pair("_ch", "home")
            .append_pair("fetch_identity", "false");
        let envelope = self.send_json(self.http.get(url)).await?;
        let value = envelope
            .data
            .ok_or_else(|| Error::Other("member response missing data".to_string()))?;
        let member_type = value
            .get("member_type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut member = MemberInfo {
            uid: value
                .get("uid")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            nickname: value
                .get("nickname")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            avatar_url: value
                .get("avatar_url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            member_type: member_type.clone(),
            mobile: value
                .get("mobile")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            total_capacity: value
                .get("total_capacity")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            use_capacity: value
                .get("use_capacity")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            expire_at: member_expire_at(&value, &member_type),
        };

        // The member endpoint does not return the profile; the account page does.
        // Profile failures must not break membership/capacity refreshes.
        if let Ok(profile) = self.account_profile().await {
            if !profile.nickname.is_empty() {
                member.nickname = profile.nickname;
            }
            if !profile.avatar_url.is_empty() {
                member.avatar_url = profile.avatar_url;
            }
        }
        if member.uid.is_empty() {
            if let Some(uid) = self.session().get("__uid") {
                member.uid = uid.to_string();
            }
        }
        Ok(member)
    }

    async fn account_profile(&self) -> Result<AccountProfile> {
        let mut url = Url::parse(&format!(
            "{}/account/info",
            self.config.account_base.trim_end_matches('/')
        ))?;
        url.query_pairs_mut()
            .append_pair("fr", "pc")
            .append_pair("platform", "pc");
        let req = self
            .http
            .get(url)
            .header(COOKIE, self.cookie_header())
            .header(ORIGIN, REFERER_VALUE)
            .header(REFERER, REFERER_VALUE);
        let response = req.send().await?;
        let status = response.status();
        self.update_session(response.headers());
        if !status.is_success() {
            return Err(Error::api(
                status.as_u16() as i64,
                0,
                "account info request failed".to_string(),
            ));
        }
        let value: Value = response.json().await?;
        let data = value.get("data").cloned().unwrap_or(Value::Null);
        let nickname = data
            .get("nickname")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let avatar_url = data
            .get("avatarUri")
            .or_else(|| data.get("avatar_url"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let avatar_url = if let Some(rest) = avatar_url.strip_prefix("http://") {
            format!("https://{rest}")
        } else {
            avatar_url.to_string()
        };
        Ok(AccountProfile {
            nickname,
            avatar_url,
        })
    }

    pub async fn list_dir(
        &self,
        pdir_fid: &str,
        page: u32,
        size: u32,
        sort: &str,
    ) -> Result<ListPage> {
        let mut url = self.api_url("/file/sort")?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs
                .append_pair("pdir_fid", pdir_fid)
                .append_pair("_page", &page.to_string())
                .append_pair("_size", &size.to_string())
                .append_pair("_fetch_total", "1")
                .append_pair("_fetch_sub_dirs", "0")
                .append_pair("_sort", sort);
        }
        let envelope = self.send_json(self.http.get(url)).await?;
        let data = envelope
            .data
            .ok_or_else(|| Error::Other("list response missing data".to_string()))?;
        let entries: Vec<FileEntry> =
            serde_json::from_value(data.get("list").cloned().unwrap_or_else(|| json!([])))?;
        let total = envelope
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("_total"))
            .and_then(value_to_u64)
            .unwrap_or(entries.len() as u64);
        let size = envelope
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("_count"))
            .and_then(value_to_u64)
            .unwrap_or(entries.len() as u64) as u32;
        Ok(ListPage {
            entries,
            total,
            page,
            size,
        })
    }

    pub async fn list_recycle(&self, page: u32, size: u32) -> Result<RecyclePage> {
        let mut url = self.api_url("/file/recycle/list")?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs
                .append_pair("_page", &page.to_string())
                .append_pair("_size", &size.to_string())
                .append_pair("_fetch_total", "1")
                .append_pair("_sort", "recycle_at:desc");
        }
        let envelope = self.send_json(self.http.get(url)).await?;
        let data = envelope
            .data
            .ok_or_else(|| Error::Other("recycle list response missing data".to_string()))?;
        let entries: Vec<crate::models::RecycleItem> =
            serde_json::from_value(data.get("list").cloned().unwrap_or_else(|| json!([])))?;
        let total = data
            .get("total")
            .and_then(value_to_u64)
            .unwrap_or(entries.len() as u64);
        let size = envelope
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("_count"))
            .and_then(value_to_u64)
            .unwrap_or(entries.len() as u64) as u32;
        Ok(RecyclePage {
            entries,
            total,
            page,
            size,
        })
    }

    pub async fn restore_recycle(&self, record_ids: &[String]) -> Result<()> {
        self.recycle_action("/file/recycle/recover", record_ids)
            .await
    }

    pub async fn delete_recycle(&self, record_ids: &[String]) -> Result<()> {
        self.recycle_action("/file/recycle/remove", record_ids)
            .await
    }

    async fn recycle_action(&self, path: &str, record_ids: &[String]) -> Result<()> {
        if record_ids.is_empty() {
            return Ok(());
        }
        let url = self.api_url(path)?;
        let body = json!({
            "select_mode": 2,
            "record_list": record_ids,
        });
        self.send_json(self.http.post(url).json(&body))
            .await
            .map(|_| ())
    }

    pub async fn mkdir(&self, parent_fid: &str, name: &str) -> Result<String> {
        let url = self.api_url("/file")?;
        let body = json!({
            "dir_init_lock": false,
            "dir_path": "",
            "file_name": name,
            "pdir_fid": parent_fid,
        });
        let envelope = self.send_json(self.http.post(url).json(&body)).await?;
        let data = envelope
            .data
            .ok_or_else(|| Error::Other("mkdir response missing data".to_string()))?;
        Ok(data
            .get("fid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    pub async fn rename(&self, fid: &str, name: &str) -> Result<()> {
        let url = self.pc_api_url("/file/rename")?;
        let body = json!({ "fid": fid, "file_name": name });
        self.send_json(self.http.post(url).json(&body))
            .await
            .map(|_| ())
    }

    pub async fn move_files(&self, fids: &[String], to_pdir_fid: &str) -> Result<()> {
        let url = self.api_url("/file/move")?;
        let body = json!({
            "action_type": 1,
            "exclude_fids": [],
            "filelist": fids,
            "to_pdir_fid": to_pdir_fid,
        });
        self.send_json(self.http.post(url).json(&body))
            .await
            .map(|_| ())
    }

    pub async fn delete_files(&self, fids: &[String], permanent: bool) -> Result<()> {
        let url = self.api_url("/file/delete")?;
        let body = json!({
            "action_type": if permanent { 2 } else { 1 },
            "exclude_fids": [],
            "filelist": fids,
        });
        self.send_json(self.http.post(url).json(&body))
            .await
            .map(|_| ())
    }

    pub async fn download_urls(&self, fids: &[String]) -> Result<Vec<DownloadUrl>> {
        let url = self.api_url("/file/download")?;
        let body = json!({ "fids": fids });
        let envelope = self.send_json(self.http.post(url).json(&body)).await?;
        let data = envelope
            .data
            .ok_or_else(|| Error::Other("download response missing data".to_string()))?;
        Ok(serde_json::from_value(data)?)
    }

    pub async fn download_range(
        &self,
        url: &str,
        start: u64,
        end_inclusive: Option<u64>,
    ) -> Result<Response> {
        let cookie = self.cookie_header();
        let range = match end_inclusive {
            Some(end) => format!("bytes={start}-{end}"),
            None => format!("bytes={start}-"),
        };
        let response = self
            .http
            .get(url)
            .header(COOKIE, cookie)
            .header(REFERER, REFERER_VALUE)
            .header(USER_AGENT, &self.config.user_agent)
            .header(reqwest::header::RANGE, range)
            .send()
            .await?;
        self.update_session(response.headers());
        Ok(response)
    }

    pub async fn upload_pre(
        &self,
        file_name: &str,
        size: i64,
        parent_fid: &str,
        mime_type: &str,
    ) -> Result<UploadPre> {
        let url = self.api_url("/file/upload/pre")?;
        let now = now_millis();
        let body = json!({
            "file_name": file_name,
            "size": size,
            "format_type": mime_type,
            "ccp_hash_update": true,
            "l_created_at": now,
            "l_updated_at": now,
            "pdir_fid": parent_fid,
            "dir_name": "",
            "parallel_upload": false,
        });
        let envelope = self.send_json(self.http.post(url).json(&body)).await?;
        let data = envelope
            .data
            .ok_or_else(|| Error::Other("upload pre response missing data".to_string()))?;
        let pre_data: UploadPreData = serde_json::from_value(data)?;
        let metadata: UploadPreMetadata = envelope
            .metadata
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        Ok(UploadPre {
            data: pre_data,
            metadata,
        })
    }

    pub async fn update_hash(&self, md5: &str, sha1: &str, task_id: &str) -> Result<bool> {
        let url = self.api_url("/file/update/hash")?;
        let body = json!({ "md5": md5, "sha1": sha1, "task_id": task_id });
        let envelope = self.send_json(self.http.post(url).json(&body)).await?;
        let data = envelope.data.unwrap_or(Value::Null);
        Ok(data.get("finish").and_then(Value::as_bool).unwrap_or(false))
    }

    pub async fn upload_auth(
        &self,
        auth_info: &str,
        auth_meta: &str,
        task_id: &str,
    ) -> Result<String> {
        let url = self.api_url("/file/upload/auth")?;
        let body = json!({
            "auth_info": auth_info,
            "auth_meta": auth_meta,
            "task_id": task_id,
        });
        let envelope = self.send_json(self.http.post(url).json(&body)).await?;
        let data = envelope
            .data
            .ok_or_else(|| Error::Other("upload auth response missing data".to_string()))?;
        data.get("auth_key")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| Error::Other("upload auth response missing auth_key".to_string()))
    }

    pub async fn upload_part(
        &self,
        pre: &UploadPre,
        part_number: u32,
        mime_type: &str,
        time_str: &str,
        body: Bytes,
    ) -> Result<String> {
        let auth_meta = part_auth_meta(
            mime_type,
            time_str,
            &pre.data.bucket,
            &pre.data.obj_key,
            part_number,
            &pre.data.upload_id,
        );
        let auth_key = self
            .upload_auth(&pre.data.auth_info, &auth_meta, &pre.data.task_id)
            .await?;
        let url = self.part_url(pre, part_number)?;
        let response = self
            .http
            .put(url)
            .header("Authorization", auth_key)
            .header(CONTENT_TYPE, mime_type)
            .header("x-oss-date", time_str)
            .header("x-oss-user-agent", OSS_USER_AGENT)
            .header(REFERER, "https://pan.quark.cn/")
            .body(body)
            .send()
            .await?;
        let status = response.status();
        let headers = response.headers().clone();
        self.update_session(&headers);
        if status != StatusCode::OK {
            let text = response.text().await.unwrap_or_default();
            return Err(Error::api(
                status.as_u16() as i64,
                0,
                format!("上传分片 {part_number} 失败: {text}"),
            ));
        }
        headers
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| Error::Other("上传分片响应缺少 ETag".to_string()))
    }

    pub async fn complete_multipart(&self, pre: &UploadPre, etags: &[String]) -> Result<()> {
        if etags.is_empty() {
            return Ok(());
        }
        let time_str = fmt_http_date(SystemTime::now());
        let xml = build_complete_xml(etags);
        let content_md5 = {
            let mut hasher = Md5::new();
            hasher.update(xml.as_bytes());
            base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                hasher.finalize(),
            )
        };
        let callback_json = serde_json::to_vec(&pre.data.callback)?;
        let callback_b64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, callback_json);
        let auth_meta = format!(
            "POST\n{content_md5}\napplication/xml\n{time_str}\nx-oss-callback:{callback_b64}\nx-oss-date:{time_str}\nx-oss-user-agent:{OSS_USER_AGENT}\n/{}/{}?uploadId={}",
            pre.data.bucket, pre.data.obj_key, pre.data.upload_id
        );
        let auth_key = self
            .upload_auth(&pre.data.auth_info, &auth_meta, &pre.data.task_id)
            .await?;
        let url = self.complete_url(pre)?;
        let response = self
            .http
            .post(url)
            .header("Authorization", auth_key)
            .header("Content-MD5", content_md5)
            .header(CONTENT_TYPE, "application/xml")
            .header("x-oss-callback", callback_b64)
            .header("x-oss-date", time_str)
            .header("x-oss-user-agent", OSS_USER_AGENT)
            .header(REFERER, "https://pan.quark.cn/")
            .body(xml)
            .send()
            .await?;
        let status = response.status();
        if status != StatusCode::OK {
            let text = response.text().await.unwrap_or_default();
            return Err(Error::api(
                status.as_u16() as i64,
                0,
                format!("合并上传分片失败: {text}"),
            ));
        }
        Ok(())
    }

    pub async fn upload_finish(&self, obj_key: &str, task_id: &str) -> Result<()> {
        let url = self.api_url("/file/upload/finish")?;
        let body = json!({ "obj_key": obj_key, "task_id": task_id });
        self.send_json(self.http.post(url).json(&body))
            .await
            .map(|_| ())
    }

    pub fn part_auth_meta(
        &self,
        pre: &UploadPre,
        part_number: u32,
        mime_type: &str,
        time_str: &str,
    ) -> String {
        part_auth_meta(
            mime_type,
            time_str,
            &pre.data.bucket,
            &pre.data.obj_key,
            part_number,
            &pre.data.upload_id,
        )
    }

    pub fn part_url(&self, pre: &UploadPre, part_number: u32) -> Result<Url> {
        let base = if let Some(base) = &self.config.oss_base_override {
            format!(
                "{}/{}/{}",
                base.trim_end_matches('/'),
                pre.data.bucket,
                pre.data.obj_key.trim_start_matches('/')
            )
        } else {
            let raw = pre.data.upload_url.trim();
            let host = raw
                .strip_prefix("https://")
                .or_else(|| raw.strip_prefix("http://"))
                .unwrap_or(raw)
                .trim_end_matches('/');
            format!(
                "https://{}.{}/{}",
                pre.data.bucket,
                host,
                pre.data.obj_key.trim_start_matches('/')
            )
        };
        let mut url = Url::parse(&base)?;
        url.query_pairs_mut()
            .append_pair("partNumber", &part_number.to_string())
            .append_pair("uploadId", &pre.data.upload_id);
        Ok(url)
    }

    fn complete_url(&self, pre: &UploadPre) -> Result<Url> {
        let mut url = self.part_url_without_query(pre)?;
        url.query_pairs_mut()
            .append_pair("uploadId", &pre.data.upload_id);
        Ok(url)
    }

    fn part_url_without_query(&self, pre: &UploadPre) -> Result<Url> {
        let base = if let Some(base) = &self.config.oss_base_override {
            format!(
                "{}/{}/{}",
                base.trim_end_matches('/'),
                pre.data.bucket,
                pre.data.obj_key.trim_start_matches('/')
            )
        } else {
            let raw = pre.data.upload_url.trim();
            let host = raw
                .strip_prefix("https://")
                .or_else(|| raw.strip_prefix("http://"))
                .unwrap_or(raw)
                .trim_end_matches('/');
            format!(
                "https://{}.{}/{}",
                pre.data.bucket,
                host,
                pre.data.obj_key.trim_start_matches('/')
            )
        };
        Ok(Url::parse(&base)?)
    }

    fn api_url(&self, path: &str) -> Result<Url> {
        self.build_url(&self.config.api_base, path)
    }

    fn pc_api_url(&self, path: &str) -> Result<Url> {
        self.build_url(&self.config.pc_api_base, path)
    }

    fn build_url(&self, base: &str, path: &str) -> Result<Url> {
        let mut url = Url::parse(&format!(
            "{}/{}",
            base.trim_end_matches('/'),
            path.trim_start_matches('/')
        ))?;
        url.query_pairs_mut()
            .append_pair("pr", "ucpro")
            .append_pair("fr", "pc");
        Ok(url)
    }

    fn cookie_header(&self) -> String {
        self.session
            .lock()
            .expect("session mutex poisoned")
            .cookie_header()
    }

    fn update_session(&self, headers: &HeaderMap) {
        self.session
            .lock()
            .expect("session mutex poisoned")
            .apply_headers(headers);
    }

    async fn send_json(&self, req: RequestBuilder) -> Result<Envelope> {
        let response = req
            .header(COOKIE, self.cookie_header())
            .header(ORIGIN, REFERER_VALUE)
            .header(REFERER, REFERER_VALUE)
            .send()
            .await?;
        let status = response.status();
        self.update_session(response.headers());
        let bytes = response.bytes().await?;
        let envelope = serde_json::from_slice::<Envelope>(&bytes).ok();
        if !status.is_success() {
            if let Some(envelope) = envelope {
                let mut err = Error::api(envelope.status as i64, envelope.code, envelope.message);
                if status == StatusCode::UNAUTHORIZED {
                    err = Error::AuthRequired;
                }
                return Err(err);
            }
            return Err(Error::api(
                status.as_u16() as i64,
                0,
                String::from_utf8_lossy(&bytes).to_string(),
            ));
        }
        let envelope = envelope.ok_or_else(|| {
            Error::Other(format!(
                "服务端返回了非 JSON 内容: {}",
                String::from_utf8_lossy(&bytes)
            ))
        })?;
        if envelope.status >= 400 || envelope.code != 0 {
            return Err(Error::api(
                envelope.status as i64,
                envelope.code as i64,
                envelope.message,
            ));
        }
        Ok(envelope)
    }
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    status: u16,
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    metadata: Option<Value>,
}

fn value_to_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

pub fn part_auth_meta(
    mime_type: &str,
    time_str: &str,
    bucket: &str,
    obj_key: &str,
    part_number: u32,
    upload_id: &str,
) -> String {
    format!(
        "PUT\n\n{mime_type}\n{time_str}\nx-oss-date:{time_str}\nx-oss-user-agent:{OSS_USER_AGENT}\n/{bucket}/{obj_key}?partNumber={part_number}&uploadId={upload_id}"
    )
}

pub fn build_complete_xml(etags: &[String]) -> String {
    let mut xml =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<CompleteMultipartUpload>\n");
    for (index, etag) in etags.iter().enumerate() {
        xml.push_str(&format!(
            "<Part>\n<PartNumber>{}</PartNumber>\n<ETag>{}</ETag>\n</Part>\n",
            index + 1,
            etag
        ));
    }
    xml.push_str("</CompleteMultipartUpload>");
    xml
}

pub fn header_value(value: &str) -> HeaderValue {
    HeaderValue::from_str(value).unwrap_or_else(|_| HeaderValue::from_static(""))
}

pub fn set_cookie_headers(headers: &HeaderMap) -> impl Iterator<Item = &HeaderValue> {
    headers.get_all(SET_COOKIE).iter()
}

#[derive(Debug, Clone, Default)]
struct AccountProfile {
    nickname: String,
    avatar_url: String,
}

fn member_expire_at(value: &Value, member_type: &str) -> Option<i64> {
    let kind = member_type.to_ascii_uppercase();
    let keys: &[&str] = if kind.contains("SVIP") {
        &["exp_svip_exp_at", "exp_at", "super_vip_exp_at"]
    } else {
        &["exp_at", "super_vip_exp_at", "exp_svip_exp_at"]
    };
    keys.iter()
        .filter_map(|key| value.get(*key).and_then(value_to_i64))
        .find(|expire_at| *expire_at > 0)
        .map(epoch_to_millis)
}

fn value_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn epoch_to_millis(value: i64) -> i64 {
    if value.abs() < 100_000_000_000 {
        value.saturating_mul(1000)
    } else {
        value
    }
}
