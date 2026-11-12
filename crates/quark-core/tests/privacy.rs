use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap};
use axum::routing::{get, post};
use axum::{Json, Router};
use quark_core::{AppCore, CoreConfig, FileSecretStore};
use quark_store::{AccountRow, Store};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Semaphore;

struct ResponseGate {
    entered: Semaphore,
    released: Semaphore,
}

impl ResponseGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Semaphore::new(0),
            released: Semaphore::new(0),
        })
    }

    async fn wait(&self) {
        self.entered.add_permits(1);
        self.released.acquire().await.unwrap().forget();
    }

    async fn started(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }

    fn release(&self) {
        self.released.add_permits(1);
    }
}

#[derive(Default)]
struct MockState {
    unlock_gate: Option<Arc<ResponseGate>>,
    list_gate: Option<Arc<ResponseGate>>,
    fail_unlock: bool,
    fail_list: bool,
    requests: Vec<(String, HashMap<String, String>)>,
}

struct PrivacyServer {
    base_url: String,
    state: Arc<Mutex<MockState>>,
    task: tokio::task::JoinHandle<()>,
}

impl PrivacyServer {
    async fn start() -> Self {
        let state = Arc::new(Mutex::new(MockState::default()));
        let app = Router::new()
            .route(
                "/1/clouddrive/member",
                get(|| async { Json(json!({"status": 200, "code": 0, "data": {"uid": "first"}})) }),
            )
            .route("/account/info", get(|| async { Json(json!({"data": {}})) }))
            .route(
                "/1/clouddrive/secret/check/register",
                post(|| async {
                    Json(json!({"status": 200, "code": 0, "data": {"has_register": true}}))
                }),
            )
            .route("/1/clouddrive/secret/v2/login", post(unlock))
            .route("/1/clouddrive/file/sort", get(list))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            base_url: format!("http://{address}"),
            state,
            task,
        }
    }

    fn config(&self, temp: &TempDir) -> CoreConfig {
        CoreConfig {
            data_dir: temp.path().join("data"),
            download_dir: temp.path().join("downloads"),
            api_base: format!("{}/1/clouddrive", self.base_url),
            pc_api_base: format!("{}/1/clouddrive", self.base_url),
            account_base: Some(self.base_url.clone()),
            ..CoreConfig::default()
        }
    }

    async fn setup(&self, temp: &TempDir) -> (AppCore, Store) {
        let config = self.config(temp);
        let core = AppCore::new(config.clone()).await.unwrap();
        let store = Store::connect(config.data_dir.join("quark-drive.db"))
            .await
            .unwrap();
        let secrets = FileSecretStore::new(config.data_dir.join("secrets.json"));
        for account_id in ["first", "second"] {
            let cookie_ref = format!("account:{account_id}");
            store
                .upsert_account(&AccountRow {
                    id: account_id.to_string(),
                    uid: account_id.to_string(),
                    cookie_ref: cookie_ref.clone(),
                    ..AccountRow::default()
                })
                .await
                .unwrap();
            secrets
                .set(&cookie_ref, &format!("__pus={account_id}"))
                .await
                .unwrap();
        }
        (core, store)
    }
}

impl Drop for PrivacyServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn unlock(
    State(state): State<Arc<Mutex<MockState>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    assert_eq!(body, json!({"secret_pwd": "246810"}));
    let (gate, fail) = {
        let state = state.lock().unwrap();
        (state.unlock_gate.clone(), state.fail_unlock)
    };
    if let Some(gate) = gate {
        gate.wait().await;
    }
    if fail {
        return Json(
            json!({"status": 400, "code": 24005, "message": "private-token private-password"}),
        );
    }
    let cookie = headers[header::COOKIE].to_str().unwrap();
    let token = if cookie.contains("__pus=first") {
        "test-private-token-first"
    } else {
        "test-private-token-second"
    };
    Json(json!({"status": 200, "code": 0, "data": {"succ": true, "token": token}}))
}

async fn list(
    State(state): State<Arc<Mutex<MockState>>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Json<Value> {
    let token = headers["X-U-Drive-Secret-Auth"]
        .to_str()
        .unwrap()
        .to_string();
    let (gate, fail) = {
        let mut state = state.lock().unwrap();
        state.requests.push((token.clone(), query.clone()));
        (state.list_gate.clone(), state.fail_list)
    };
    if token.ends_with("first") {
        if let Some(gate) = gate {
            gate.wait().await;
        }
    }
    if fail {
        return Json(
            json!({"status": 400, "code": 24005, "message": "private-token private-file-name"}),
        );
    }
    Json(json!({
        "status": 200,
        "code": 0,
        "data": {"list": [{
            "fid": "private-file-id",
            "pdir_fid": query["pdir_fid"],
            "file_name": "private-file-name.txt",
            "file": true,
            "size": 42
        }]},
        "metadata": {"_total": 12, "_count": 1}
    }))
}

#[tokio::test]
async fn browsing_is_paginated_read_only_and_memory_only() {
    let server = PrivacyServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, store) = server.setup(&temp).await;
    let mut events = core.subscribe();
    let secret_path = core.data_dir().join("secrets.json");
    let original_secrets = std::fs::read(&secret_path).unwrap();
    assert!(core.privacy_registered("first").await.unwrap());
    core.privacy_unlock("first", "246810").await.unwrap();
    let page = core
        .privacy_list_dir("first", "folder", 0, 500)
        .await
        .unwrap();
    assert_eq!(page.entries[0].name, "private-file-name.txt");
    assert_eq!(page.entries[0].parent_fid, "folder");
    assert_eq!(page.total, 12);
    assert_eq!(page.page, 1);
    core.privacy_list_dir("first", "folder", 2, 0)
        .await
        .unwrap();
    {
        let state = server.state.lock().unwrap();
        assert_eq!(state.requests.len(), 2);
        assert_eq!(state.requests[0].1["_page"], "1");
        assert_eq!(state.requests[0].1["_size"], "200");
        assert_eq!(state.requests[1].1["_page"], "2");
        assert_eq!(state.requests[1].1["_size"], "1");
    }
    assert!(store
        .list_cached_files("first", "folder")
        .await
        .unwrap()
        .is_empty());
    assert_eq!(std::fs::read(secret_path).unwrap(), original_secrets);
    assert!(matches!(
        events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    for entry in std::fs::read_dir(core.data_dir()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        let content = String::from_utf8_lossy(&bytes);
        assert!(!content.contains("private-token"));
        assert!(!content.contains("private-file-name"));
        assert!(!content.contains("246810"));
    }
    let restarted = AppCore::new(server.config(&temp)).await.unwrap();
    assert!(restarted
        .privacy_list_dir("first", "folder", 1, 10)
        .await
        .is_err());
    core.privacy_lock("first").await.unwrap();
    assert!(core
        .privacy_list_dir("first", "folder", 1, 10)
        .await
        .is_err());
    assert_eq!(server.state.lock().unwrap().requests.len(), 2);
}

#[tokio::test]
async fn lock_cancels_unlock_before_any_session_is_installed() {
    let server = PrivacyServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, _) = server.setup(&temp).await;
    let gate = ResponseGate::new();
    server.state.lock().unwrap().unlock_gate = Some(gate.clone());
    let pending_core = core.clone();
    let pending = tokio::spawn(async move { pending_core.privacy_unlock("first", "246810").await });
    gate.started().await;
    core.privacy_lock("first").await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    gate.release();
    assert!(core.privacy_list_dir("first", "0", 1, 20).await.is_err());
    server.state.lock().unwrap().unlock_gate = None;
    core.privacy_unlock("first", "246810").await.unwrap();
    core.privacy_list_dir("first", "0", 1, 20).await.unwrap();
}

#[tokio::test]
async fn logout_cancels_unlock_and_prevents_late_session_insertion() {
    let server = PrivacyServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, store) = server.setup(&temp).await;
    let gate = ResponseGate::new();
    server.state.lock().unwrap().unlock_gate = Some(gate.clone());
    let pending_core = core.clone();
    let pending = tokio::spawn(async move { pending_core.privacy_unlock("first", "246810").await });
    gate.started().await;
    core.logout("first").await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    gate.release();
    assert!(store.get_account("first").await.unwrap().is_none());
    assert!(core.privacy_unlock("first", "246810").await.is_err());
    assert!(core.privacy_list_dir("first", "0", 1, 20).await.is_err());
    core.privacy_lock("first").await.unwrap();
    core.privacy_lock("never-present").await.unwrap();
    server.state.lock().unwrap().unlock_gate = None;
    core.login_with_cookie("__pus=first").await.unwrap();
    core.privacy_unlock("first", "246810").await.unwrap();
    core.privacy_list_dir("first", "0", 1, 20).await.unwrap();
}

#[tokio::test]
async fn newer_unlock_supersedes_pending_unlock_without_losing_its_session() {
    let server = PrivacyServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, _) = server.setup(&temp).await;
    let gate = ResponseGate::new();
    server.state.lock().unwrap().unlock_gate = Some(gate.clone());
    let pending_core = core.clone();
    let pending = tokio::spawn(async move { pending_core.privacy_unlock("first", "246810").await });
    gate.started().await;
    server.state.lock().unwrap().unlock_gate = None;
    core.privacy_unlock("first", "246810").await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    gate.release();
    core.privacy_list_dir("first", "0", 1, 20).await.unwrap();
}

#[tokio::test]
async fn lock_cancels_late_listing_without_affecting_other_accounts() {
    let server = PrivacyServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, _) = server.setup(&temp).await;
    core.privacy_unlock("first", "246810").await.unwrap();
    core.privacy_unlock("second", "246810").await.unwrap();
    let gate = ResponseGate::new();
    server.state.lock().unwrap().list_gate = Some(gate.clone());
    let pending_core = core.clone();
    let pending =
        tokio::spawn(async move { pending_core.privacy_list_dir("first", "0", 1, 20).await });
    gate.started().await;
    core.privacy_lock("first").await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    gate.release();
    let page = core.privacy_list_dir("second", "0", 1, 20).await.unwrap();
    assert_eq!(page.entries.len(), 1);
    let state = server.state.lock().unwrap();
    assert_eq!(state.requests[0].0, "test-private-token-first");
    assert_eq!(state.requests[1].0, "test-private-token-second");
}

#[tokio::test]
async fn logout_revokes_existing_session_and_pending_listing() {
    let server = PrivacyServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, _) = server.setup(&temp).await;
    core.privacy_unlock("first", "246810").await.unwrap();
    core.privacy_unlock("second", "246810").await.unwrap();
    let gate = ResponseGate::new();
    server.state.lock().unwrap().list_gate = Some(gate.clone());
    let pending_core = core.clone();
    let pending =
        tokio::spawn(async move { pending_core.privacy_list_dir("first", "0", 1, 20).await });
    gate.started().await;
    core.logout("first").await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    gate.release();
    assert!(core.privacy_list_dir("first", "0", 1, 20).await.is_err());
    core.privacy_list_dir("second", "0", 1, 20).await.unwrap();
}

#[tokio::test]
async fn privacy_errors_are_fixed_and_listing_failure_locks_session() {
    let server = PrivacyServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, _) = server.setup(&temp).await;
    let mut events = core.subscribe();
    server.state.lock().unwrap().fail_unlock = true;
    let error = core.privacy_unlock("first", "246810").await.unwrap_err();
    assert_eq!(error.to_string(), "隐私空间解锁失败，请检查密码后重试");
    server.state.lock().unwrap().fail_unlock = false;
    core.privacy_unlock("first", "246810").await.unwrap();
    server.state.lock().unwrap().fail_list = true;
    let error = core
        .privacy_list_dir("first", "0", 1, 20)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "隐私空间读取失败，请重新解锁后重试");
    server.state.lock().unwrap().fail_list = false;
    assert!(core.privacy_list_dir("first", "0", 1, 20).await.is_err());
    assert_eq!(server.state.lock().unwrap().requests.len(), 1);
    assert!(matches!(
        events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    let error = core
        .privacy_registered("missing-account")
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "无法检查隐私空间状态，请稍后重试");
}
