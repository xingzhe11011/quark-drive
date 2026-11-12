mod fs;
mod privacy;
mod privacy_transfer;
mod secrets;
mod transfer;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use quark_protocol::{
    ClientConfig, FileEntry, QrLogin, QrLoginConfig, QrLoginPoll, QuarkClient, RecycleItem,
    Session, QR_LOGIN_TTL_MS, ROOT_FID,
};
use quark_store::{now_millis, AccountRow, FileCacheRow, Store, TransferRow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::{broadcast, RwLock, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub use fs::{sanitize_file_name, unique_path};
pub use secrets::{FileSecretStore, SecretError};

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("protocol error: {0}")]
    Protocol(#[from] quark_protocol::Error),
    #[error("store error: {0}")]
    Store(#[from] quark_store::StoreError),
    #[error("secret error: {0}")]
    Secret(#[from] secrets::SecretError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("account not found: {0}")]
    AccountNotFound(String),
    #[error("transfer not found: {0}")]
    TransferNotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Clone)]
pub struct CoreConfig {
    pub data_dir: PathBuf,
    pub download_dir: PathBuf,
    pub api_base: String,
    pub pc_api_base: String,
    pub account_base: Option<String>,
    pub oss_base_override: Option<String>,
    pub user_agent: Option<String>,
    pub qr_login: QrLoginConfig,
}

impl Default for CoreConfig {
    fn default() -> Self {
        let download_dir = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("QuarkDrive");
        Self {
            data_dir: dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("quark-drive"),
            download_dir,
            api_base: "https://drive.quark.cn/1/clouddrive".to_string(),
            pc_api_base: "https://drive-pc.quark.cn/1/clouddrive".to_string(),
            account_base: None,
            oss_base_override: None,
            user_agent: None,
            qr_login: QrLoginConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub uid: String,
    pub nickname: String,
    pub avatar_url: String,
    pub vip_type: String,
    pub vip_expire_at: Option<i64>,
    pub is_vip: bool,
    pub total_space: i64,
    pub used_space: i64,
    pub last_check_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub fid: String,
    pub parent_fid: String,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub category: i64,
    pub format_type: String,
    pub updated_at: i64,
    pub thumbnail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResult {
    pub entries: Vec<FileView>,
    pub total: u64,
    pub page: u32,
    pub size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecycleView {
    pub record_id: String,
    pub fid: String,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub category: i64,
    pub format_type: String,
    pub parent_fid: String,
    pub deleted_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecycleListResult {
    pub entries: Vec<RecycleView>,
    pub total: u64,
    pub page: u32,
    pub size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferView {
    pub id: String,
    pub account_id: String,
    pub kind: String,
    pub status: String,
    pub phase: String,
    pub fid: Option<String>,
    pub local_path: String,
    pub remote_name: String,
    pub remote_parent_fid: String,
    pub total_bytes: i64,
    pub done_bytes: i64,
    pub speed_bps: f64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub download_dir: String,
    pub concurrency: u32,
    pub max_speed_bps: u64,
    pub theme: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub fid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub accounts: Vec<AccountView>,
    pub settings: Settings,
    pub transfers: Vec<TransferView>,
    pub default_parent_fid: String,
    pub data_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrLoginStart {
    pub session_id: String,
    pub qr_url: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrLoginPollView {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CoreEvent {
    AccountsChanged {
        accounts: Vec<AccountView>,
    },
    TransferProgress {
        id: String,
        status: String,
        phase: String,
        done_bytes: i64,
        total_bytes: i64,
        speed_bps: f64,
    },
    TransferChanged {
        transfer: Box<TransferView>,
    },
    LoginState {
        state: String,
        message: String,
    },
    Toast {
        level: String,
        message: String,
    },
}

#[derive(Clone)]
pub struct AppCore {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    secrets: FileSecretStore,
    config: CoreConfig,
    events: broadcast::Sender<CoreEvent>,
    clients: RwLock<HashMap<String, QuarkClient>>,
    privacy: Mutex<privacy::PrivacySessions>,
    privacy_transfers: Mutex<privacy_transfer::PrivacyTransfers>,
    qr_logins: RwLock<HashMap<String, QrLoginSession>>,
    active: Mutex<HashSet<String>>,
    rerun: Mutex<HashSet<String>>,
    cancels: Mutex<HashMap<String, CancellationToken>>,
    semaphore: Mutex<Arc<Semaphore>>,
    settings: RwLock<Settings>,
    speeds: Mutex<HashMap<String, SpeedState>>,
}

struct QrLoginSession {
    login: QrLogin,
    expires_at: i64,
}

#[derive(Debug, Clone, Copy)]
struct SpeedState {
    at: Instant,
    done: i64,
    speed_bps: f64,
}

impl AppCore {
    pub async fn new(config: CoreConfig) -> Result<Self> {
        std::fs::create_dir_all(&config.data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&config.data_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        std::fs::create_dir_all(&config.download_dir)?;
        let store = Store::connect(config.data_dir.join("quark-drive.db")).await?;
        let secret_path = std::env::var_os("QUARK_DRIVE_SECRET_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| config.data_dir.join("secrets.json"));
        let secrets = FileSecretStore::new(secret_path);
        let settings = load_settings(&store, &config).await?;
        let concurrency = settings.concurrency.clamp(1, 6) as usize;
        let (events, _) = broadcast::channel(1024);
        let core = Self {
            inner: Arc::new(Inner {
                store,
                secrets,
                config,
                events,
                clients: RwLock::new(HashMap::new()),
                privacy: Mutex::new(privacy::PrivacySessions::default()),
                privacy_transfers: Mutex::new(privacy_transfer::PrivacyTransfers::default()),
                qr_logins: RwLock::new(HashMap::new()),
                active: Mutex::new(HashSet::new()),
                rerun: Mutex::new(HashSet::new()),
                cancels: Mutex::new(HashMap::new()),
                semaphore: Mutex::new(Arc::new(Semaphore::new(concurrency))),
                settings: RwLock::new(settings),
                speeds: Mutex::new(HashMap::new()),
            }),
        };
        core.recover_transfers().await?;
        Ok(core)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.inner.events.subscribe()
    }

    pub fn emit(&self, event: CoreEvent) {
        let _ = self.inner.events.send(event);
    }

    pub fn data_dir(&self) -> &Path {
        &self.inner.config.data_dir
    }

    pub async fn bootstrap(&self) -> Result<Bootstrap> {
        Ok(Bootstrap {
            accounts: self.accounts().await?,
            settings: self.settings().await,
            transfers: self.list_transfers(200).await?,
            default_parent_fid: ROOT_FID.to_string(),
            data_dir: self.data_dir().to_string_lossy().to_string(),
        })
    }

    pub async fn accounts(&self) -> Result<Vec<AccountView>> {
        Ok(self
            .inner
            .store
            .list_accounts()
            .await?
            .iter()
            .map(account_view)
            .collect())
    }

    pub async fn settings(&self) -> Settings {
        self.inner.settings.read().await.clone()
    }

    pub async fn login_with_cookie(&self, cookie: &str) -> Result<AccountView> {
        let session = Session::from_cookie_header(cookie)?;
        let client = QuarkClient::new(session.clone(), self.client_config())?;
        let member = client.member_info().await?;
        let current_cookie = client.session().cookie_header();
        let accounts = self.inner.store.list_accounts().await?;
        let id = if member.uid.is_empty() {
            let digest = Sha256::digest(current_cookie.as_bytes());
            format!("local-{}", hex_prefix(&digest, 12))
        } else {
            accounts
                .iter()
                .find(|row| row.uid == member.uid)
                .map(|row| row.id.clone())
                .or_else(|| {
                    accounts
                        .iter()
                        .find(|row| row.uid.is_empty() && row.id.starts_with("local-"))
                        .map(|row| row.id.clone())
                })
                .unwrap_or_else(|| member.uid.clone())
        };
        let key = format!("account:{id}");
        self.inner.secrets.set(&key, &current_cookie).await?;
        let existing = accounts.into_iter().find(|row| row.id == id);
        let now = now_millis();
        let row = AccountRow {
            id: id.clone(),
            uid: member.uid,
            nickname: member.nickname,
            avatar_url: member.avatar_url,
            vip_type: member.member_type,
            vip_expire_at: member.expire_at,
            total_space: member.total_capacity,
            used_space: member.use_capacity,
            cookie_ref: key,
            created_at: existing.as_ref().map(|row| row.created_at).unwrap_or(now),
            updated_at: now,
            last_check_at: Some(now),
        };
        self.inner.store.upsert_account(&row).await?;
        self.inner.clients.write().await.insert(id.clone(), client);
        self.cancel_privacy_transfers(&id);
        self.inner
            .privacy
            .lock()
            .expect("privacy mutex poisoned")
            .login(&id);
        let view = account_view(&row);
        self.emit(CoreEvent::AccountsChanged {
            accounts: self.accounts().await?,
        });
        info!(account = %id, "account logged in");
        Ok(view)
    }

    pub async fn start_qr_login(&self) -> Result<QrLoginStart> {
        let now = now_millis();
        self.inner
            .qr_logins
            .write()
            .await
            .retain(|_, session| session.expires_at > now);

        let login = QrLogin::start(self.inner.config.qr_login.clone()).await?;
        let session_id = uuid::Uuid::new_v4().to_string();
        let expires_at = now + QR_LOGIN_TTL_MS;
        let start = QrLoginStart {
            session_id: session_id.clone(),
            qr_url: login.qr_url().to_string(),
            expires_at,
        };
        self.inner
            .qr_logins
            .write()
            .await
            .insert(session_id, QrLoginSession { login, expires_at });
        Ok(start)
    }

    pub async fn poll_qr_login(&self, session_id: &str) -> Result<QrLoginPollView> {
        let session = {
            let sessions = self.inner.qr_logins.read().await;
            sessions
                .get(session_id)
                .map(|session| (session.login.clone(), session.expires_at))
        };
        let Some((login, expires_at)) = session else {
            return Err(CoreError::InvalidInput(
                "登录二维码已失效，请刷新二维码".to_string(),
            ));
        };
        if now_millis() >= expires_at {
            self.inner.qr_logins.write().await.remove(session_id);
            return Ok(QrLoginPollView {
                state: "expired".to_string(),
                expires_at: Some(expires_at),
                account: None,
                message: Some("二维码已过期，请刷新".to_string()),
            });
        }

        match login.poll().await? {
            QrLoginPoll::Waiting => Ok(QrLoginPollView {
                state: "waiting".to_string(),
                expires_at: Some(expires_at),
                account: None,
                message: None,
            }),
            QrLoginPoll::Expired => {
                self.inner.qr_logins.write().await.remove(session_id);
                Ok(QrLoginPollView {
                    state: "expired".to_string(),
                    expires_at: Some(expires_at),
                    account: None,
                    message: Some("二维码已过期，请刷新".to_string()),
                })
            }
            QrLoginPoll::Success(session) => {
                self.inner.qr_logins.write().await.remove(session_id);
                let account = self.login_with_cookie(&session.cookie_header()).await?;
                Ok(QrLoginPollView {
                    state: "success".to_string(),
                    expires_at: None,
                    account: Some(account),
                    message: Some("登录成功".to_string()),
                })
            }
        }
    }

    pub async fn cancel_qr_login(&self, session_id: &str) -> Result<()> {
        self.inner.qr_logins.write().await.remove(session_id);
        Ok(())
    }

    pub async fn logout(&self, account_id: &str) -> Result<()> {
        self.cancel_privacy_transfers(account_id);
        self.inner
            .privacy
            .lock()
            .expect("privacy mutex poisoned")
            .logout(account_id);
        if let Some(row) = self.inner.store.get_account(account_id).await? {
            let _ = self.inner.secrets.delete(&row.cookie_ref).await;
        }
        self.inner.clients.write().await.remove(account_id);
        self.inner.store.delete_account(account_id).await?;
        self.emit(CoreEvent::AccountsChanged {
            accounts: self.accounts().await?,
        });
        Ok(())
    }

    pub async fn refresh_account(&self, account_id: &str) -> Result<AccountView> {
        let client = self.client_for(account_id).await?;
        let member = client.member_info().await?;
        self.persist_session(account_id, &client).await?;
        let mut row = self
            .inner
            .store
            .get_account(account_id)
            .await?
            .ok_or_else(|| CoreError::AccountNotFound(account_id.to_string()))?;
        if !member.nickname.is_empty() {
            row.nickname = member.nickname;
        }
        if !member.avatar_url.is_empty() {
            row.avatar_url = member.avatar_url;
        }
        row.vip_type = member.member_type;
        row.vip_expire_at = member.expire_at;
        if !member.uid.is_empty() {
            row.uid = member.uid;
        }
        row.total_space = member.total_capacity;
        row.used_space = member.use_capacity;
        row.updated_at = now_millis();
        row.last_check_at = Some(now_millis());
        self.inner.store.upsert_account(&row).await?;
        let view = account_view(&row);
        self.emit(CoreEvent::AccountsChanged {
            accounts: self.accounts().await?,
        });
        Ok(view)
    }

    pub async fn list_dir(
        &self,
        account_id: &str,
        parent_fid: &str,
        page: u32,
        size: u32,
    ) -> Result<ListResult> {
        let client = self.client_for(account_id).await?;
        let result = client
            .list_dir(
                parent_fid,
                page.max(1),
                size.clamp(1, 200),
                quark_protocol::DEFAULT_SORT,
            )
            .await?;
        self.persist_session(account_id, &client).await?;
        if page <= 1 {
            let cached = result
                .entries
                .iter()
                .map(|entry| file_cache_row(account_id, parent_fid, entry))
                .collect::<Vec<_>>();
            self.inner
                .store
                .replace_file_page(account_id, parent_fid, &cached)
                .await?;
        }
        Ok(ListResult {
            entries: result.entries.iter().map(file_view).collect(),
            total: result.total,
            page: result.page,
            size: result.size,
        })
    }

    pub async fn mkdir(&self, account_id: &str, parent_fid: &str, name: &str) -> Result<String> {
        let name = self.ensure_new_name(account_id, parent_fid, name).await?;
        let client = self.client_for(account_id).await?;
        let fid = client.mkdir(parent_fid, &name).await?;
        self.persist_session(account_id, &client).await?;
        Ok(fid)
    }

    pub async fn rename(&self, account_id: &str, fid: &str, name: &str) -> Result<()> {
        let client = self.client_for(account_id).await?;
        client.rename(fid, name).await?;
        self.persist_session(account_id, &client).await?;
        Ok(())
    }

    pub async fn move_files(
        &self,
        account_id: &str,
        fids: &[String],
        parent_fid: &str,
    ) -> Result<()> {
        let client = self.client_for(account_id).await?;
        client.move_files(fids, parent_fid).await?;
        self.persist_session(account_id, &client).await?;
        Ok(())
    }

    pub async fn delete_files(
        &self,
        account_id: &str,
        fids: &[String],
        permanent: bool,
    ) -> Result<()> {
        let client = self.client_for(account_id).await?;
        client.delete_files(fids, permanent).await?;
        self.persist_session(account_id, &client).await?;
        Ok(())
    }

    pub async fn list_recycle(
        &self,
        account_id: &str,
        page: u32,
        size: u32,
    ) -> Result<RecycleListResult> {
        let client = self.client_for(account_id).await?;
        let result = client.list_recycle(page, size).await?;
        self.persist_session(account_id, &client).await?;
        Ok(RecycleListResult {
            entries: result.entries.iter().map(recycle_view).collect(),
            total: result.total,
            page: result.page,
            size: result.size,
        })
    }

    pub async fn restore_recycle(&self, account_id: &str, record_ids: &[String]) -> Result<()> {
        let client = self.client_for(account_id).await?;
        client.restore_recycle(record_ids).await?;
        self.persist_session(account_id, &client).await?;
        Ok(())
    }

    pub async fn delete_recycle(&self, account_id: &str, record_ids: &[String]) -> Result<()> {
        let client = self.client_for(account_id).await?;
        client.delete_recycle(record_ids).await?;
        self.persist_session(account_id, &client).await?;
        Ok(())
    }

    pub async fn clear_recycle(&self, account_id: &str) -> Result<u64> {
        let client = self.client_for(account_id).await?;
        let mut removed = 0_u64;
        for _ in 0..20 {
            let page = client.list_recycle(1, 100).await?;
            if page.entries.is_empty() {
                break;
            }
            let ids: Vec<String> = page
                .entries
                .iter()
                .map(|item| item.record_id.clone())
                .filter(|id| !id.is_empty())
                .collect();
            if ids.is_empty() {
                break;
            }
            removed += ids.len() as u64;
            client.delete_recycle(&ids).await?;
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(300)).await;
                let check = client.list_recycle(1, 100).await?;
                if check
                    .entries
                    .iter()
                    .all(|item| !ids.contains(&item.record_id))
                {
                    break;
                }
            }
        }
        self.persist_session(account_id, &client).await?;
        Ok(removed)
    }

    pub async fn cached_files(&self, account_id: &str, parent_fid: &str) -> Result<Vec<FileView>> {
        Ok(self
            .inner
            .store
            .list_cached_files(account_id, parent_fid)
            .await?
            .iter()
            .map(|row| FileView {
                fid: row.fid.clone(),
                parent_fid: row.pdir_fid.clone(),
                name: row.name.clone(),
                is_dir: row.is_dir,
                size: row.size,
                category: row.category,
                format_type: String::new(),
                updated_at: row.updated_at,
                thumbnail: String::new(),
            })
            .collect())
    }

    pub async fn enqueue_upload(
        &self,
        account_id: &str,
        paths: Vec<String>,
        parent_fid: &str,
    ) -> Result<Vec<String>> {
        if paths.is_empty() {
            return Err(CoreError::InvalidInput("没有选择文件".to_string()));
        }
        let client = self.client_for(account_id).await?;
        let mut created = Vec::new();
        for path in paths {
            let source = PathBuf::from(path);
            if !source.exists() {
                return Err(CoreError::InvalidInput(format!(
                    "本地路径不存在: {}",
                    source.display()
                )));
            }
            if source.is_file() {
                let name = source
                    .file_name()
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| CoreError::InvalidInput("文件名无效".to_string()))?
                    .to_string();
                let name = self.ensure_new_name(account_id, parent_fid, &name).await?;
                let size = source.metadata()?.len() as i64;
                let row = make_upload_row(account_id, parent_fid, &source, &name, size);
                self.inner.store.insert_transfer(&row).await?;
                created.push(row.id.clone());
                self.spawn_transfer(row.id);
            } else if source.is_dir() {
                let root_name = source
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("未命名文件夹");
                let remote_root = self.ensure_dir(account_id, parent_fid, root_name).await?;
                let entries = tokio::task::spawn_blocking({
                    let source = source.clone();
                    move || fs::collect_local_tree(&source)
                })
                .await
                .map_err(|err| CoreError::Other(err.to_string()))??;

                let mut dir_map: HashMap<PathBuf, String> = HashMap::new();
                dir_map.insert(PathBuf::new(), remote_root.clone());
                let mut directories = entries
                    .iter()
                    .filter(|entry| entry.is_dir)
                    .collect::<Vec<_>>();
                directories.sort_by_key(|entry| entry.relative.components().count());
                for entry in directories {
                    let parent_rel = entry.relative.parent().unwrap_or(Path::new(""));
                    let remote_parent = dir_map
                        .get(parent_rel)
                        .cloned()
                        .unwrap_or_else(|| remote_root.clone());
                    let name = entry
                        .relative
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("未命名文件夹");
                    let fid = self.ensure_dir(account_id, &remote_parent, name).await?;
                    dir_map.insert(entry.relative.clone(), fid);
                }
                for entry in entries.iter().filter(|entry| !entry.is_dir) {
                    let parent_rel = entry.relative.parent().unwrap_or(Path::new(""));
                    let remote_parent = dir_map
                        .get(parent_rel)
                        .cloned()
                        .unwrap_or_else(|| remote_root.clone());
                    let name = entry
                        .relative
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("未命名文件")
                        .to_string();
                    let name = self
                        .ensure_new_name(account_id, &remote_parent, &name)
                        .await?;
                    let row = make_upload_row(
                        account_id,
                        &remote_parent,
                        &entry.absolute,
                        &name,
                        entry.size as i64,
                    );
                    self.inner.store.insert_transfer(&row).await?;
                    created.push(row.id.clone());
                    self.spawn_transfer(row.id);
                }
            }
        }
        self.persist_session(account_id, &client).await?;
        Ok(created)
    }

    pub async fn enqueue_download(
        &self,
        account_id: &str,
        items: Vec<DownloadItem>,
        dest_dir: Option<String>,
    ) -> Result<Vec<String>> {
        if items.is_empty() {
            return Err(CoreError::InvalidInput("没有选择文件".to_string()));
        }
        let default_dir = self.inner.settings.read().await.download_dir.clone();
        let base = dest_dir.unwrap_or(default_dir);
        let base = PathBuf::from(base);
        std::fs::create_dir_all(&base)?;
        let client = self.client_for(account_id).await?;
        let mut remote_files = Vec::new();
        let mut remote_dirs = Vec::new();
        for item in items {
            if item.is_dir {
                let name = sanitize_file_name(&item.name);
                let relative = PathBuf::from(&name);
                remote_dirs.push(relative.clone());
                self.collect_remote_dir(
                    &client,
                    &item.fid,
                    &relative,
                    &mut remote_files,
                    &mut remote_dirs,
                )
                .await?;
            } else {
                remote_files.push(RemoteFile {
                    fid: item.fid,
                    name: sanitize_file_name(&item.name),
                    size: item.size,
                    relative: PathBuf::from(sanitize_file_name(&item.name)),
                });
            }
        }
        remote_dirs.sort_by_key(|path| path.components().count());
        for relative in remote_dirs {
            std::fs::create_dir_all(base.join(relative))?;
        }
        let mut created = Vec::new();
        for file in remote_files {
            let target = unique_path(&base.join(&file.relative));
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let now = now_millis();
            let row = TransferRow {
                id: uuid::Uuid::new_v4().to_string(),
                account_id: account_id.to_string(),
                kind: "download".to_string(),
                status: "queued".to_string(),
                phase: "queued".to_string(),
                fid: Some(file.fid),
                local_path: target.to_string_lossy().to_string(),
                remote_name: file.name,
                remote_parent_fid: ROOT_FID.to_string(),
                total_bytes: file.size,
                done_bytes: 0,
                error_code: None,
                error_msg: None,
                session_json: None,
                priority: 10,
                created_at: now,
                updated_at: now,
            };
            self.inner.store.insert_transfer(&row).await?;
            created.push(row.id.clone());
            self.spawn_transfer(row.id);
        }
        self.persist_session(account_id, &client).await?;
        Ok(created)
    }

    pub async fn list_transfers(&self, limit: i64) -> Result<Vec<TransferView>> {
        let rows = self.inner.store.list_transfers(limit).await?;
        let speeds = self.inner.speeds.lock().expect("speeds mutex poisoned");
        let mut views = rows
            .iter()
            .map(|row| {
                transfer_view(
                    row,
                    speeds
                        .get(&row.id)
                        .map(|state| state.speed_bps)
                        .unwrap_or(0.0),
                )
            })
            .collect::<Vec<_>>();
        drop(speeds);
        views.extend(self.privacy_transfer_views());
        views.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.created_at.cmp(&left.created_at))
        });
        views.truncate(limit.max(0) as usize);
        Ok(views)
    }

    pub async fn pause_transfer(&self, id: &str) -> Result<()> {
        if privacy_transfer::is_privacy_transfer(id) {
            return self.pause_privacy_transfer(id);
        }
        self.inner
            .store
            .patch_transfer_state(
                id,
                "paused",
                "paused",
                self.done_bytes(id).await?,
                None,
                None,
            )
            .await?;
        if let Some(token) = self
            .inner
            .cancels
            .lock()
            .expect("cancels mutex poisoned")
            .get(id)
        {
            token.cancel();
        }
        self.emit_transfer_changed(id).await?;
        Ok(())
    }

    pub async fn resume_transfer(&self, id: &str) -> Result<()> {
        if privacy_transfer::is_privacy_transfer(id) {
            return self.resume_privacy_transfer(id);
        }
        let row = self
            .inner
            .store
            .get_transfer(id)
            .await?
            .ok_or_else(|| CoreError::TransferNotFound(id.to_string()))?;
        if matches!(row.status.as_str(), "done" | "canceled") {
            return Err(CoreError::InvalidInput("该任务已经结束".to_string()));
        }
        self.inner
            .store
            .patch_transfer_state(id, "queued", "queued", row.done_bytes, None, None)
            .await?;
        self.spawn_transfer(id.to_string());
        self.emit_transfer_changed(id).await?;
        Ok(())
    }

    pub async fn retry_transfer(&self, id: &str) -> Result<()> {
        self.resume_transfer(id).await
    }

    pub async fn cancel_transfer(&self, id: &str) -> Result<()> {
        if privacy_transfer::is_privacy_transfer(id) {
            return self.cancel_privacy_transfer(id);
        }
        self.inner
            .store
            .patch_transfer_state(
                id,
                "canceled",
                "canceled",
                self.done_bytes(id).await?,
                Some(("canceled", "已取消")),
                None,
            )
            .await?;
        if let Some(token) = self
            .inner
            .cancels
            .lock()
            .expect("cancels mutex poisoned")
            .get(id)
        {
            token.cancel();
        }
        self.emit_transfer_changed(id).await?;
        Ok(())
    }

    pub async fn remove_transfer(&self, id: &str) -> Result<()> {
        if privacy_transfer::is_privacy_transfer(id) {
            return self.remove_privacy_transfer(id);
        }
        self.cancel_transfer(id).await?;
        self.inner.store.delete_transfer(id).await?;
        Ok(())
    }

    pub async fn clear_finished_transfers(&self) -> Result<()> {
        self.inner.store.clear_finished_transfers().await?;
        self.clear_finished_privacy_transfers();
        Ok(())
    }

    pub async fn update_settings(
        &self,
        download_dir: Option<String>,
        concurrency: Option<u32>,
        max_speed_bps: Option<u64>,
        theme: Option<String>,
    ) -> Result<Settings> {
        let mut settings = self.inner.settings.write().await;
        if let Some(dir) = download_dir {
            if !dir.trim().is_empty() {
                std::fs::create_dir_all(&dir)?;
                settings.download_dir = dir;
            }
        }
        if let Some(value) = concurrency {
            settings.concurrency = value.clamp(1, 6);
        }
        if let Some(value) = max_speed_bps {
            settings.max_speed_bps = value;
        }
        if let Some(value) = theme {
            settings.theme = value;
        }
        let snapshot = settings.clone();
        drop(settings);
        self.inner
            .store
            .set_setting("download_dir", &snapshot.download_dir)
            .await?;
        self.inner
            .store
            .set_setting("concurrency", &snapshot.concurrency.to_string())
            .await?;
        self.inner
            .store
            .set_setting("max_speed_bps", &snapshot.max_speed_bps.to_string())
            .await?;
        self.inner
            .store
            .set_setting("theme", &snapshot.theme)
            .await?;
        *self
            .inner
            .semaphore
            .lock()
            .expect("semaphore mutex poisoned") =
            Arc::new(Semaphore::new(snapshot.concurrency as usize));
        Ok(snapshot)
    }

    pub async fn client_for(&self, account_id: &str) -> Result<QuarkClient> {
        if let Some(client) = self.inner.clients.read().await.get(account_id).cloned() {
            return Ok(client);
        }
        let account = self
            .inner
            .store
            .get_account(account_id)
            .await?
            .ok_or_else(|| CoreError::AccountNotFound(account_id.to_string()))?;
        let raw = self.inner.secrets.require(&account.cookie_ref).await?;
        let session = Session::from_cookie_header(&raw)?;
        let client = QuarkClient::new(session, self.client_config())?;
        self.inner
            .clients
            .write()
            .await
            .insert(account_id.to_string(), client.clone());
        Ok(client)
    }

    pub async fn persist_session(&self, account_id: &str, client: &QuarkClient) -> Result<()> {
        let account = self
            .inner
            .store
            .get_account(account_id)
            .await?
            .ok_or_else(|| CoreError::AccountNotFound(account_id.to_string()))?;
        let cookie = client.session().cookie_header();
        self.inner.secrets.set(&account.cookie_ref, &cookie).await?;
        Ok(())
    }

    pub(crate) async fn emit_transfer_changed(&self, id: &str) -> Result<()> {
        if let Some(row) = self.inner.store.get_transfer(id).await? {
            let speed = self
                .inner
                .speeds
                .lock()
                .expect("speeds mutex poisoned")
                .get(id)
                .map(|state| state.speed_bps)
                .unwrap_or(0.0);
            self.emit(CoreEvent::TransferChanged {
                transfer: Box::new(transfer_view(&row, speed)),
            });
        }
        Ok(())
    }

    pub(crate) async fn update_progress(
        &self,
        id: &str,
        status: &str,
        phase: &str,
        done_bytes: i64,
        total_bytes: i64,
    ) -> Result<()> {
        let now = Instant::now();
        let speed = {
            let mut speeds = self.inner.speeds.lock().expect("speeds mutex poisoned");
            let state = speeds.entry(id.to_string()).or_insert(SpeedState {
                at: now,
                done: 0,
                speed_bps: 0.0,
            });
            let elapsed = now.duration_since(state.at).as_secs_f64();
            if elapsed >= 0.4 {
                let delta = (done_bytes - state.done).max(0) as f64;
                state.speed_bps = delta / elapsed;
                state.done = done_bytes;
                state.at = now;
            }
            state.speed_bps
        };

        self.inner
            .store
            .patch_transfer_state(id, status, phase, done_bytes, None, None)
            .await?;
        self.emit(CoreEvent::TransferProgress {
            id: id.to_string(),
            status: status.to_string(),
            phase: phase.to_string(),
            done_bytes,
            total_bytes,
            speed_bps: speed,
        });
        Ok(())
    }

    pub(crate) async fn finish_transfer(
        &self,
        id: &str,
        result: Result<()>,
        token: CancellationToken,
    ) {
        let current = self.inner.store.get_transfer(id).await.ok().flatten();
        let done = current.as_ref().map(|row| row.done_bytes).unwrap_or(0);
        let total = current.as_ref().map(|row| row.total_bytes).unwrap_or(0);
        if result.is_ok() {
            let _ = self
                .inner
                .store
                .patch_transfer_state(id, "done", "done", total.max(done), None, None)
                .await;
        } else if !token.is_cancelled() {
            let message = result
                .as_ref()
                .err()
                .map(ToString::to_string)
                .unwrap_or_default();
            let _ = self
                .inner
                .store
                .patch_transfer_state(
                    id,
                    "failed",
                    "failed",
                    done,
                    Some(("error", &message)),
                    None,
                )
                .await;
        }
        self.inner
            .active
            .lock()
            .expect("active mutex poisoned")
            .remove(id);
        self.inner
            .cancels
            .lock()
            .expect("cancels mutex poisoned")
            .remove(id);
        self.inner
            .speeds
            .lock()
            .expect("speeds mutex poisoned")
            .remove(id);
        let rerun = self
            .inner
            .rerun
            .lock()
            .expect("rerun mutex poisoned")
            .remove(id);
        if rerun {
            let status = self
                .inner
                .store
                .get_transfer(id)
                .await
                .ok()
                .flatten()
                .map(|row| row.status);
            if status.as_deref() == Some("queued") {
                self.spawn_transfer(id.to_string());
            }
        }
        if let Err(err) = &result {
            if !token.is_cancelled() {
                warn!(transfer = %id, error = %err, "transfer failed");
            }
        }
        let _ = self.emit_transfer_changed(id).await;
    }

    pub(crate) fn spawn_transfer(&self, id: String) {
        {
            let mut active = self.inner.active.lock().expect("active mutex poisoned");
            if !active.insert(id.clone()) {
                self.inner
                    .rerun
                    .lock()
                    .expect("rerun mutex poisoned")
                    .insert(id);
                return;
            }
        }
        let token = CancellationToken::new();
        self.inner
            .cancels
            .lock()
            .expect("cancels mutex poisoned")
            .insert(id.clone(), token.clone());
        let core = self.clone();
        let semaphore = self
            .inner
            .semaphore
            .lock()
            .expect("semaphore mutex poisoned")
            .clone();
        tokio::spawn(async move {
            let _permit = semaphore.acquire().await;
            let result = transfer::run_transfer(core.clone(), id.clone(), token.clone()).await;
            core.finish_transfer(&id, result, token).await;
        });
    }

    async fn recover_transfers(&self) -> Result<()> {
        for row in self.inner.store.recoverable_transfers().await? {
            if row.status == "paused" {
                continue;
            }
            self.inner
                .store
                .patch_transfer_state(&row.id, "queued", "queued", row.done_bytes, None, None)
                .await?;
            self.spawn_transfer(row.id);
        }
        Ok(())
    }

    async fn done_bytes(&self, id: &str) -> Result<i64> {
        Ok(self
            .inner
            .store
            .get_transfer(id)
            .await?
            .map(|row| row.done_bytes)
            .unwrap_or(0))
    }

    fn client_config(&self) -> ClientConfig {
        let mut config = ClientConfig {
            api_base: self.inner.config.api_base.clone(),
            pc_api_base: self.inner.config.pc_api_base.clone(),
            account_base: self
                .inner
                .config
                .account_base
                .clone()
                .unwrap_or_else(|| "https://pan.quark.cn".to_string()),
            oss_base_override: self.inner.config.oss_base_override.clone(),
            ..ClientConfig::default()
        };
        if let Some(ua) = &self.inner.config.user_agent {
            config.user_agent = ua.clone();
        }
        config
    }

    async fn ensure_dir(&self, account_id: &str, parent_fid: &str, name: &str) -> Result<String> {
        let name = sanitize_file_name(name);
        if let Some(fid) = self.find_child_dir(account_id, parent_fid, &name).await? {
            return Ok(fid);
        }
        let client = self.client_for(account_id).await?;
        match client.mkdir(parent_fid, &name).await {
            Ok(fid) => {
                self.persist_session(account_id, &client).await?;
                Ok(fid)
            }
            Err(err) => {
                if let Some(fid) = self.find_child_dir(account_id, parent_fid, &name).await? {
                    return Ok(fid);
                }
                Err(err.into())
            }
        }
    }

    async fn ensure_new_name(
        &self,
        account_id: &str,
        parent_fid: &str,
        preferred: &str,
    ) -> Result<String> {
        let preferred = sanitize_file_name(preferred);
        let client = self.client_for(account_id).await?;
        let page = client
            .list_dir(parent_fid, 1, 200, quark_protocol::DEFAULT_SORT)
            .await?;
        if !page
            .entries
            .iter()
            .any(|entry| entry.file_name == preferred)
        {
            return Ok(preferred);
        }
        let (stem, ext) = split_extension(&preferred);
        for index in 1..1000 {
            let candidate = if ext.is_empty() {
                format!("{stem} ({index})")
            } else {
                format!("{stem} ({index}).{ext}")
            };
            if !page
                .entries
                .iter()
                .any(|entry| entry.file_name == candidate)
            {
                return Ok(candidate);
            }
        }
        Err(CoreError::Other("无法生成不冲突的文件名".to_string()))
    }

    async fn find_child_dir(
        &self,
        account_id: &str,
        parent_fid: &str,
        name: &str,
    ) -> Result<Option<String>> {
        let client = self.client_for(account_id).await?;
        let page = client
            .list_dir(parent_fid, 1, 200, quark_protocol::DEFAULT_SORT)
            .await?;
        Ok(page
            .entries
            .into_iter()
            .find(|entry| entry.file_name == name && entry.is_dir())
            .map(|entry| entry.fid))
    }

    fn collect_remote_dir<'a>(
        &'a self,
        client: &'a QuarkClient,
        fid: &'a str,
        relative: &'a Path,
        out: &'a mut Vec<RemoteFile>,
        out_dirs: &'a mut Vec<PathBuf>,
    ) -> futures_util::future::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut page_no = 1;
            loop {
                let page = client
                    .list_dir(fid, page_no, 200, quark_protocol::DEFAULT_SORT)
                    .await?;
                for entry in &page.entries {
                    let name = sanitize_file_name(&entry.file_name);
                    if entry.is_dir() {
                        let child = relative.join(&name);
                        out_dirs.push(child.clone());
                        self.collect_remote_dir(client, &entry.fid, &child, out, out_dirs)
                            .await?;
                    } else {
                        out.push(RemoteFile {
                            fid: entry.fid.clone(),
                            name,
                            size: entry.size,
                            relative: relative.join(sanitize_file_name(&entry.file_name)),
                        });
                    }
                }
                if page.entries.is_empty()
                    || page.entries.len() < page.size as usize
                    || (page_no * page.size) as u64 >= page.total
                {
                    break;
                }
                page_no += 1;
            }
            Ok(())
        })
    }
}

#[derive(Debug)]
struct RemoteFile {
    fid: String,
    name: String,
    size: i64,
    relative: PathBuf,
}

async fn load_settings(store: &Store, config: &CoreConfig) -> Result<Settings> {
    let download_dir = store
        .get_setting("download_dir")
        .await?
        .unwrap_or_else(|| config.download_dir.to_string_lossy().to_string());
    let concurrency = store
        .get_setting("concurrency")
        .await?
        .and_then(|value| value.parse().ok())
        .unwrap_or(2);
    let max_speed_bps = store
        .get_setting("max_speed_bps")
        .await?
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let theme = store
        .get_setting("theme")
        .await?
        .unwrap_or_else(|| "system".to_string());
    Ok(Settings {
        download_dir,
        concurrency,
        max_speed_bps,
        theme,
    })
}

fn account_view(row: &AccountRow) -> AccountView {
    AccountView {
        id: row.id.clone(),
        uid: row.uid.clone(),
        nickname: if row.nickname.is_empty() {
            "夸克用户".to_string()
        } else {
            row.nickname.clone()
        },
        avatar_url: row.avatar_url.clone(),
        vip_type: row.vip_type.clone(),
        vip_expire_at: row.vip_expire_at,
        is_vip: {
            let kind = row.vip_type.to_ascii_uppercase();
            kind.contains("VIP") || kind.contains("SUPER") || kind.contains("MEMBER")
        },
        total_space: row.total_space,
        used_space: row.used_space,
        last_check_at: row.last_check_at,
    }
}

fn file_view(entry: &FileEntry) -> FileView {
    FileView {
        fid: entry.fid.clone(),
        parent_fid: entry.pdir_fid.clone(),
        name: entry.file_name.clone(),
        is_dir: entry.is_dir(),
        size: entry.size,
        category: entry.category,
        format_type: entry.format_type.clone(),
        updated_at: entry.updated_at,
        thumbnail: entry.thumbnail.clone(),
    }
}

fn recycle_view(item: &RecycleItem) -> RecycleView {
    RecycleView {
        record_id: item.record_id.clone(),
        fid: item.fid.clone(),
        name: item.file_name.clone(),
        is_dir: item.is_dir(),
        size: item.size,
        category: item.category,
        format_type: item.format_type.clone(),
        parent_fid: item.pdir_fid.clone(),
        deleted_at: item.move_recycle_at,
        updated_at: item.updated_at,
    }
}

fn file_cache_row(account_id: &str, parent_fid: &str, entry: &FileEntry) -> FileCacheRow {
    FileCacheRow {
        account_id: account_id.to_string(),
        fid: entry.fid.clone(),
        pdir_fid: if entry.pdir_fid.is_empty() {
            parent_fid.to_string()
        } else {
            entry.pdir_fid.clone()
        },
        name: entry.file_name.clone(),
        is_dir: entry.is_dir(),
        size: entry.size,
        category: entry.category,
        updated_at: entry.updated_at,
        cached_at: now_millis(),
    }
}

fn transfer_view(row: &TransferRow, speed_bps: f64) -> TransferView {
    TransferView {
        id: row.id.clone(),
        account_id: row.account_id.clone(),
        kind: row.kind.clone(),
        status: row.status.clone(),
        phase: row.phase.clone(),
        fid: row.fid.clone(),
        local_path: row.local_path.clone(),
        remote_name: row.remote_name.clone(),
        remote_parent_fid: row.remote_parent_fid.clone(),
        total_bytes: row.total_bytes,
        done_bytes: row.done_bytes,
        speed_bps,
        error_code: row.error_code.clone(),
        error_message: row.error_msg.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn make_upload_row(
    account_id: &str,
    parent_fid: &str,
    local_path: &Path,
    remote_name: &str,
    size: i64,
) -> TransferRow {
    let now = now_millis();
    TransferRow {
        id: uuid::Uuid::new_v4().to_string(),
        account_id: account_id.to_string(),
        kind: "upload".to_string(),
        status: "queued".to_string(),
        phase: "queued".to_string(),
        fid: None,
        local_path: local_path.to_string_lossy().to_string(),
        remote_name: remote_name.to_string(),
        remote_parent_fid: parent_fid.to_string(),
        total_bytes: size,
        done_bytes: 0,
        error_code: None,
        error_msg: None,
        session_json: None,
        priority: 5,
        created_at: now,
        updated_at: now,
    }
}

fn split_extension(name: &str) -> (String, String) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => {
            (stem.to_string(), ext.to_string())
        }
        _ => (name.to_string(), String::new()),
    }
}

fn hex_prefix(bytes: &[u8], length: usize) -> String {
    bytes
        .iter()
        .take(length)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
