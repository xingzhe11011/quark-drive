use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use quark_core::{AppCore, CoreConfig, CoreEvent, DownloadItem, FileSecretStore};
use quark_store::{AccountRow, Store};
use serde_json::{json, Value};
use tempfile::TempDir;

#[derive(Default)]
struct MockState {
    base_url: String,
    move_auth: Option<String>,
    move_body: Option<Value>,
    download_auth: Option<String>,
    cdn_has_secret_header: Option<bool>,
    upload_pre_auth: Option<String>,
    upload_pre_name: Option<String>,
    mkdir_names: Vec<String>,
    part_has_secret_header: Option<bool>,
    upload_bytes: Vec<u8>,
    finish_auth: Option<String>,
    download_delay_ms: u64,
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
            .route("/1/clouddrive/member", get(member))
            .route("/account/info", get(account_info))
            .route("/1/clouddrive/secret/v2/login", post(unlock))
            .route("/1/clouddrive/file/sort", get(list))
            .route("/1/clouddrive/file", post(mkdir))
            .route("/1/clouddrive/secret/file/move", post(move_files))
            .route("/1/clouddrive/file/download", post(download_url))
            .route("/1/clouddrive/file/upload/pre", post(upload_pre))
            .route("/1/clouddrive/file/update/hash", post(update_hash))
            .route("/1/clouddrive/file/upload/auth", post(upload_auth))
            .route("/1/clouddrive/file/upload/finish", post(upload_finish))
            .route("/cdn/private", get(cdn_file))
            .route("/oss/{*path}", put(upload_part).post(complete_upload))
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let base_url = format!("http://{address}");
        state.lock().unwrap().base_url = base_url.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            base_url,
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
            oss_base_override: Some(format!("{}/oss", self.base_url)),
            ..CoreConfig::default()
        }
    }

    async fn setup(&self, temp: &TempDir) -> (AppCore, Store) {
        let config = self.config(temp);
        let core = AppCore::new(config.clone()).await.unwrap();
        let store = Store::connect(config.data_dir.join("quark-drive.db"))
            .await
            .unwrap();
        let cookie_ref = "account:private-account".to_string();
        store
            .upsert_account(&AccountRow {
                id: "private-account".to_string(),
                uid: "private-account".to_string(),
                cookie_ref: cookie_ref.clone(),
                ..AccountRow::default()
            })
            .await
            .unwrap();
        FileSecretStore::new(config.data_dir.join("secrets.json"))
            .set(&cookie_ref, "__pus=privacy-test")
            .await
            .unwrap();
        (core, store)
    }
}

impl Drop for PrivacyServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn member() -> Json<Value> {
    Json(json!({
        "status": 200,
        "code": 0,
        "data": {"uid": "private-account"}
    }))
}

async fn account_info() -> Json<Value> {
    Json(json!({"data": {}}))
}

async fn unlock() -> Json<Value> {
    Json(json!({
        "status": 200,
        "code": 0,
        "data": {"succ": true, "token": "privacy-token"}
    }))
}

async fn list(Query(query): Query<HashMap<String, String>>) -> Json<Value> {
    let parent = query.get("pdir_fid").map(String::as_str).unwrap_or("2");
    let entries = if parent == "private-folder" {
        json!([{
            "fid": "nested-file",
            "pdir_fid": parent,
            "file_name": "nested.bin",
            "file": true,
            "dir": false,
            "size": 4
        }])
    } else {
        json!([
            {
                "fid": "private-file",
                "pdir_fid": parent,
                "file_name": "private.bin",
                "file": true,
                "dir": false,
                "size": 4
            },
            {
                "fid": "private-folder",
                "pdir_fid": parent,
                "file_name": "私密目录",
                "file": false,
                "dir": true,
                "size": 0
            },
            {
                "fid": "existing-upload",
                "pdir_fid": parent,
                "file_name": "upload.bin",
                "file": true,
                "dir": false,
                "size": 8
            }
        ])
    };
    let count = entries.as_array().map(Vec::len).unwrap_or(0);
    Json(json!({
        "status": 200,
        "code": 0,
        "data": {"list": entries},
        "metadata": {"_total": count, "_count": count}
    }))
}

async fn move_files(
    State(state): State<Arc<Mutex<MockState>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    let mut state = state.lock().unwrap();
    state.move_auth = Some(
        headers["X-U-Drive-Secret-Auth"]
            .to_str()
            .unwrap()
            .to_string(),
    );
    state.move_body = Some(body);
    Json(json!({"status": 200, "code": 0, "data": {}}))
}

async fn mkdir(State(state): State<Arc<Mutex<MockState>>>, Json(body): Json<Value>) -> Json<Value> {
    let name = body
        .get("file_name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    state.lock().unwrap().mkdir_names.push(name.clone());
    Json(json!({
        "status": 200,
        "code": 0,
        "data": {"fid": format!("dir-{name}")}
    }))
}

async fn download_url(
    State(state): State<Arc<Mutex<MockState>>>,
    headers: HeaderMap,
) -> Json<Value> {
    let base_url = {
        let mut state = state.lock().unwrap();
        state.download_auth = Some(
            headers["X-U-Drive-Secret-Auth"]
                .to_str()
                .unwrap()
                .to_string(),
        );
        state.base_url.clone()
    };
    Json(json!({
        "status": 200,
        "code": 0,
        "data": [{
            "fid": "private-file",
            "file_name": "private.bin",
            "size": 4,
            "download_url": format!("{base_url}/cdn/private")
        }]
    }))
}

async fn cdn_file(State(state): State<Arc<Mutex<MockState>>>, headers: HeaderMap) -> Response {
    let delay = {
        let mut state = state.lock().unwrap();
        state.cdn_has_secret_header = Some(headers.contains_key("X-U-Drive-Secret-Auth"));
        state.download_delay_ms
    };
    if delay > 0 {
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
    (
        StatusCode::PARTIAL_CONTENT,
        [(header::CONTENT_RANGE, "bytes 0-3/4")],
        [1_u8, 2, 3, 4],
    )
        .into_response()
}

async fn upload_pre(
    State(state): State<Arc<Mutex<MockState>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    let mut state = state.lock().unwrap();
    state.upload_pre_auth = Some(
        headers["X-U-Drive-Secret-Auth"]
            .to_str()
            .unwrap()
            .to_string(),
    );
    state.upload_pre_name = body
        .get("file_name")
        .and_then(Value::as_str)
        .map(str::to_string);
    Json(json!({
        "status": 200,
        "code": 0,
        "data": {
            "finish": false,
            "task_id": "upload-task",
            "upload_id": "upload-id",
            "auth_info": "auth-info",
            "upload_url": "ignored.invalid",
            "obj_key": "private-object",
            "bucket": "bucket",
            "callback": {
                "callbackUrl": "http://callback.invalid",
                "callbackBody": "{}"
            }
        },
        "metadata": {"part_size": 1048576}
    }))
}

async fn update_hash() -> Json<Value> {
    Json(json!({"status": 200, "code": 0, "data": {"finish": false}}))
}

async fn upload_auth() -> Json<Value> {
    Json(json!({"status": 200, "code": 0, "data": {"auth_key": "oss-key"}}))
}

async fn upload_part(
    State(state): State<Arc<Mutex<MockState>>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    {
        let mut state = state.lock().unwrap();
        state.part_has_secret_header = Some(headers.contains_key("X-U-Drive-Secret-Auth"));
        state.upload_bytes.extend_from_slice(&body);
    }
    (StatusCode::OK, [(header::ETAG, "\"part-etag\"")]).into_response()
}

async fn complete_upload() -> StatusCode {
    StatusCode::OK
}

async fn upload_finish(
    State(state): State<Arc<Mutex<MockState>>>,
    headers: HeaderMap,
) -> Json<Value> {
    state.lock().unwrap().finish_auth = Some(
        headers["X-U-Drive-Secret-Auth"]
            .to_str()
            .unwrap()
            .to_string(),
    );
    Json(json!({"status": 200, "code": 0, "data": {}}))
}

#[tokio::test]
async fn privacy_download_upload_and_move_are_end_to_end_and_memory_only() {
    let server = PrivacyServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, store) = server.setup(&temp).await;
    core.privacy_unlock("private-account", "246810")
        .await
        .unwrap();

    let download_dir = temp.path().join("private-downloads");
    let ids = core
        .privacy_enqueue_download(
            "private-account",
            vec![DownloadItem {
                fid: "private-file".to_string(),
                name: "private.bin".to_string(),
                size: 4,
                is_dir: false,
            }],
            Some(download_dir.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    assert_eq!(ids.len(), 1);
    wait_for_status(&core, &ids[0], "done").await;
    let transfer = core
        .list_transfers(20)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == ids[0])
        .unwrap();
    assert_eq!(
        tokio::fs::read(&transfer.local_path).await.unwrap(),
        [1_u8, 2, 3, 4]
    );
    assert_eq!(
        server.state.lock().unwrap().download_auth.as_deref(),
        Some("privacy-token")
    );
    assert_eq!(
        server.state.lock().unwrap().cdn_has_secret_header,
        Some(false)
    );

    let payload = vec![9_u8; 1024 * 1024 + 17];
    let source = temp.path().join("upload.bin");
    tokio::fs::write(&source, &payload).await.unwrap();
    let upload_ids = core
        .privacy_enqueue_upload(
            "private-account",
            vec![source.to_string_lossy().to_string()],
            "2",
        )
        .await
        .unwrap();
    wait_for_status(&core, &upload_ids[0], "done").await;
    {
        let state = server.state.lock().unwrap();
        assert_eq!(state.upload_pre_name.as_deref(), Some("upload (1).bin"));
        assert_eq!(state.upload_pre_auth.as_deref(), Some("privacy-token"));
        assert_eq!(state.finish_auth.as_deref(), Some("privacy-token"));
        assert_eq!(state.part_has_secret_header, Some(false));
        assert_eq!(state.upload_bytes, payload);
    }

    core.privacy_move_files(
        "private-account",
        &["private-file".to_string()],
        "private-folder",
    )
    .await
    .unwrap();
    {
        let state = server.state.lock().unwrap();
        assert_eq!(state.move_auth.as_deref(), Some("privacy-token"));
        assert_eq!(
            state.move_body.as_ref().unwrap(),
            &json!({
                "action_type": 1,
                "exclude_fids": [],
                "filelist": ["private-file"],
                "to_pdir_fid": "private-folder"
            })
        );
    }

    assert!(store.list_transfers(100).await.unwrap().is_empty());
    for entry in std::fs::read_dir(core.data_dir()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("private.bin"));
        assert!(!text.contains("privacy-token"));
        assert!(!text.contains("246810"));
    }
}

#[tokio::test]
async fn locking_redacts_and_removes_in_flight_private_transfers() {
    let server = PrivacyServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, _) = server.setup(&temp).await;
    server.state.lock().unwrap().download_delay_ms = 5_000;
    core.privacy_unlock("private-account", "246810")
        .await
        .unwrap();
    let mut events = core.subscribe();
    let ids = core
        .privacy_enqueue_download(
            "private-account",
            vec![DownloadItem {
                fid: "private-file".to_string(),
                name: "private.bin".to_string(),
                size: 4,
                is_dir: false,
            }],
            Some(temp.path().join("locked").to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    wait_for_status(&core, &ids[0], "running").await;
    core.privacy_lock("private-account").await.unwrap();

    assert!(core
        .list_transfers(20)
        .await
        .unwrap()
        .iter()
        .all(|item| !item.id.starts_with("privacy:")));
    let mut last_event = None;
    while let Ok(event) = events.try_recv() {
        if let CoreEvent::TransferChanged { transfer } = event {
            if transfer.id == ids[0] {
                last_event = Some(transfer);
            }
        }
    }
    let last_event = last_event.expect("private transfer event");
    assert_eq!(last_event.remote_name, "隐私空间任务");
    assert!(last_event.local_path.is_empty());
    assert!(core
        .privacy_list_dir("private-account", "2", 1, 20)
        .await
        .is_err());
}

#[tokio::test]
async fn privacy_folder_download_and_upload_traverse_directories() {
    let server = PrivacyServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, store) = server.setup(&temp).await;
    core.privacy_unlock("private-account", "246810")
        .await
        .unwrap();

    let destination = temp.path().join("recursive");
    let ids = core
        .privacy_enqueue_download(
            "private-account",
            vec![DownloadItem {
                fid: "private-folder".to_string(),
                name: "私密目录".to_string(),
                size: 0,
                is_dir: true,
            }],
            Some(destination.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    wait_for_status(&core, &ids[0], "done").await;
    let transfer = core
        .list_transfers(20)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == ids[0])
        .unwrap();
    assert!(transfer.local_path.ends_with("私密目录/nested.bin"));
    assert_eq!(
        tokio::fs::read(&transfer.local_path).await.unwrap(),
        [1_u8, 2, 3, 4]
    );

    let folder = temp.path().join("local-folder");
    tokio::fs::create_dir_all(folder.join("empty"))
        .await
        .unwrap();
    tokio::fs::write(folder.join("nested.txt"), b"folder upload")
        .await
        .unwrap();
    let upload_ids = core
        .privacy_enqueue_upload(
            "private-account",
            vec![folder.to_string_lossy().to_string()],
            "2",
        )
        .await
        .unwrap();
    wait_for_status(&core, &upload_ids[0], "done").await;
    {
        let state = server.state.lock().unwrap();
        assert!(state.mkdir_names.iter().any(|name| name == "local-folder"));
        assert!(state.mkdir_names.iter().any(|name| name == "empty"));
        assert_eq!(state.upload_pre_name.as_deref(), Some("nested.txt"));
        assert_eq!(state.upload_bytes, b"folder upload");
    }
    assert!(store.list_transfers(100).await.unwrap().is_empty());
}

async fn wait_for_status(core: &AppCore, id: &str, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(item) = core
            .list_transfers(100)
            .await
            .unwrap()
            .into_iter()
            .find(|item| item.id == id)
        {
            if item.status == expected {
                return;
            }
            if item.status == "failed" {
                panic!("privacy transfer failed: {item:?}");
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {expected}"
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}
