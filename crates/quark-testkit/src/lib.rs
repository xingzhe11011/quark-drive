use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde_json::{json, Value};

pub type SharedState = Arc<Mutex<FakeState>>;

#[derive(Debug)]
pub struct FakeState {
    pub list_calls: usize,
    pub download_url_calls: usize,
    pub pre_calls: usize,
    pub upload_parts: usize,
    pub upload_bytes: Vec<u8>,
    pub upload_complete: bool,
    pub upload_finished: bool,
    pub expire_next_download: bool,
    pub download_delay_ms: u64,
    pub download_bytes: Vec<u8>,
    pub fail_next_list: bool,
    pub qr_token_calls: usize,
    pub qr_poll_calls: usize,
    pub qr_account_info_calls: usize,
    pub qr_ticket_ready: bool,
    pub qr_expired: bool,
    pub recycle_items: Vec<Value>,
    pub recycle_restore_calls: usize,
    pub recycle_remove_calls: usize,
}

impl Default for FakeState {
    fn default() -> Self {
        let mut download_bytes = Vec::with_capacity(1024 * 1024);
        for index in 0..1024 * 1024 {
            download_bytes.push((index % 251) as u8);
        }
        Self {
            list_calls: 0,
            download_url_calls: 0,
            pre_calls: 0,
            upload_parts: 0,
            upload_bytes: Vec::new(),
            upload_complete: false,
            upload_finished: false,
            expire_next_download: false,
            download_delay_ms: 0,
            download_bytes,
            fail_next_list: false,
            qr_token_calls: 0,
            qr_poll_calls: 0,
            qr_account_info_calls: 0,
            qr_ticket_ready: false,
            qr_expired: false,
            recycle_items: vec![
                json!({
                    "record_id": "record-1",
                    "fid": "recycle-folder",
                    "file_name": "旧文件夹",
                    "file_type": 0,
                    "include_items": 2,
                    "category": 0,
                    "format_type": "",
                    "size": 0,
                    "pdir_fid": "0",
                    "updated_at": 1_800_000_000_000_i64,
                    "move_recycle_at": 1_810_000_000_000_i64
                }),
                json!({
                    "record_id": "record-2",
                    "fid": "recycle-file",
                    "file_name": "old.txt",
                    "file_type": 1,
                    "include_items": 0,
                    "category": 0,
                    "format_type": "txt",
                    "size": 12,
                    "pdir_fid": "0",
                    "updated_at": 1_800_000_000_000_i64,
                    "move_recycle_at": 1_810_000_000_000_i64
                }),
            ],
            recycle_restore_calls: 0,
            recycle_remove_calls: 0,
        }
    }
}

pub struct FakeQuarkServer {
    pub base_url: String,
    pub state: SharedState,
}

impl FakeQuarkServer {
    pub async fn start() -> Self {
        let state = Arc::new(Mutex::new(FakeState::default()));
        let app = Router::new()
            .route("/1/clouddrive/config", get(config))
            .route("/1/clouddrive/member", get(member))
            .route("/1/clouddrive/file/sort", get(list))
            .route("/1/clouddrive/file", post(mkdir))
            .route("/1/clouddrive/file/rename", post(rename))
            .route("/1/clouddrive/file/move", post(move_files))
            .route("/1/clouddrive/file/delete", post(delete_files))
            .route("/1/clouddrive/file/recycle/list", get(recycle_list))
            .route("/1/clouddrive/file/recycle/recover", post(recycle_recover))
            .route("/1/clouddrive/file/recycle/remove", post(recycle_remove))
            .route("/1/clouddrive/file/download", post(download_url))
            .route("/1/clouddrive/file/upload/pre", post(upload_pre))
            .route("/1/clouddrive/file/update/hash", post(update_hash))
            .route("/1/clouddrive/file/upload/auth", post(upload_auth))
            .route("/1/clouddrive/file/upload/finish", post(upload_finish))
            .route("/oss/{*path}", put(upload_part).post(complete_upload))
            .route("/download/{fid}", get(download))
            .route("/qr/token", get(qr_token))
            .route("/qr/poll", get(qr_poll))
            .route("/account/info", get(qr_account_info))
            .layer(middleware::from_fn(require_api_headers))
            .with_state(state.clone());

        let port = std::env::var("FAKE_BIND_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(0);
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .expect("bind fake server");
        let address = listener.local_addr().expect("fake server address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            base_url: format!("http://{address}"),
            state,
        }
    }

    pub fn api_base(&self) -> String {
        format!("{}/1/clouddrive", self.base_url)
    }

    pub fn oss_base(&self) -> String {
        format!("{}/oss", self.base_url)
    }

    pub fn qr_token_url(&self) -> String {
        format!("{}/qr/token", self.base_url)
    }

    pub fn qr_poll_url(&self) -> String {
        format!("{}/qr/poll", self.base_url)
    }

    pub fn qr_account_info_url(&self) -> String {
        format!("{}/account/info", self.base_url)
    }

    pub fn qr_scan_url(&self) -> String {
        format!("{}/qr/scan", self.base_url)
    }
}

async fn require_api_headers(request: Request, next: Next) -> Response {
    if request.uri().path().starts_with("/1/clouddrive") {
        let cookie = request
            .headers()
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let origin = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok());
        let referer = request
            .headers()
            .get(header::REFERER)
            .and_then(|value| value.to_str().ok());
        if !cookie.contains("__puus=") && !cookie.contains("__pus=")
            || origin != Some("https://pan.quark.cn")
            || referer != Some("https://pan.quark.cn")
        {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "status": 401,
                    "code": 31001,
                    "message": "missing quark browser headers"
                })),
            )
                .into_response();
        }
    }
    next.run(request).await
}

fn assert_cookie_contains(headers: &HeaderMap, expected: &str) {
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(
        cookie.contains(expected),
        "request cookie does not contain {expected}"
    );
}

fn assert_cas_query(query: &HashMap<String, String>, with_token: bool) {
    assert_eq!(query.get("client_id").map(String::as_str), Some("532"));
    assert_eq!(query.get("v").map(String::as_str), Some("1.2"));
    let request_id = query
        .get("request_id")
        .expect("CAS request must include request_id");
    let valid_uuid = request_id.len() == 36
        && request_id
            .chars()
            .enumerate()
            .all(|(index, ch)| match index {
                8 | 13 | 18 | 23 => ch == '-',
                _ => ch.is_ascii_hexdigit(),
            });
    assert!(valid_uuid, "request_id is not a UUID: {request_id}");
    assert_eq!(query.contains_key("token"), with_token);
}

async fn config() -> Response {
    ok(json!({}), json!({}))
}

async fn qr_token(
    State(state): State<SharedState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    assert_cas_query(&query, false);
    state.lock().expect("fake state").qr_token_calls += 1;
    (
        [(header::SET_COOKIE, "qr_prep=ready; Path=/; HttpOnly")],
        Json(json!({
            "status": 2_000_000,
            "data": {"members": {"token": "fake-qr-token"}}
        })),
    )
        .into_response()
}

async fn qr_poll(
    State(state): State<SharedState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    assert_cas_query(&query, true);
    assert_eq!(
        query.get("token").map(String::as_str),
        Some("fake-qr-token")
    );
    assert_cookie_contains(&headers, "qr_prep=ready");
    let (expired, ready) = {
        let mut guard = state.lock().expect("fake state");
        guard.qr_poll_calls += 1;
        (guard.qr_expired, guard.qr_ticket_ready)
    };
    if expired {
        return Json(json!({
            "status": 50_004_002,
            "message": "二维码已过期"
        }))
        .into_response();
    }
    if ready {
        Json(json!({
            "status": 2_000_000,
            "data": {"members": {"service_ticket": "fake-service-ticket"}}
        }))
        .into_response()
    } else {
        Json(json!({
            "status": 50_004_001,
            "message": "Query result is empty"
        }))
        .into_response()
    }
}

async fn qr_account_info(
    State(state): State<SharedState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if !query.contains_key("st") {
        return Json(json!({
            "success": true,
            "code": "OK",
            "data": {
                "nickname": "测试用户",
                "avatarUri": "http://image.quark.cn/avatar/test.png"
            }
        }))
        .into_response();
    }
    assert_eq!(
        query.get("st").map(String::as_str),
        Some("fake-service-ticket")
    );
    assert_eq!(query.get("lw").map(String::as_str), Some("scan"));
    assert_cookie_contains(&headers, "qr_prep=ready");
    state.lock().expect("fake state").qr_account_info_calls += 1;
    let mut response =
        Json(json!({"success": true, "data": {"nickname": "测试用户"}})).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("__pus=fake-pus; Path=/; SameSite=Lax"),
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("__uid=test-uid; Path=/; SameSite=Lax"),
    );
    response
}

async fn member() -> Response {
    let mut response = ok(
        json!({
            "uid": "test-uid",
            "avatar_url": "",
            "member_type": "SUPER_VIP",
            "exp_at": 1_812_470_400_000_i64,
            "exp_svip_exp_at": 1_812_470_400_000_i64,
            "total_capacity": 1024_i64 * 1024 * 1024,
            "use_capacity": 256_i64 * 1024 * 1024
        }),
        json!({}),
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("__puus=fake-puus; Path=/; HttpOnly; SameSite=Lax"),
    );
    response
}

async fn list(
    State(state): State<SharedState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let mut guard = state.lock().expect("fake state");
    guard.list_calls += 1;
    if guard.fail_next_list {
        guard.fail_next_list = false;
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"status": 429, "code": 429, "message": "rate limited"})),
        )
            .into_response();
    }
    let parent = query.get("pdir_fid").map(String::as_str).unwrap_or("0");
    let entries = if parent == "dir-1" {
        json!([
            {
                "fid": "file-in-dir",
                "pdir_fid": "dir-1",
                "file_name": "nested.txt",
                "file": true,
                "dir": false,
                "size": 11,
                "category": 0,
                "updated_at": 1_760_000_200_000_i64
            },
            {
                "fid": "empty-dir",
                "pdir_fid": "dir-1",
                "file_name": "空目录",
                "file": false,
                "dir": true,
                "size": 0,
                "category": 0,
                "updated_at": 1_760_000_300_000_i64
            }
        ])
    } else if parent == "empty-dir" {
        json!([])
    } else {
        json!([
            {
                "fid": "dir-1",
                "pdir_fid": "0",
                "file_name": "文档",
                "file": false,
                "dir": true,
                "size": 0,
                "category": 0,
                "updated_at": 1_760_000_000_000_i64
            },
            {
                "fid": "file-1",
                "pdir_fid": "0",
                "file_name": "hello.txt",
                "file": true,
                "dir": false,
                "size": 11,
                "category": 0,
                "updated_at": 1_760_000_100_000_i64
            }
        ])
    };
    let total = entries.as_array().map(Vec::len).unwrap_or(0);
    let body = json!({
        "status": 200,
        "code": 0,
        "message": "ok",
        "data": {
            "list": entries
        },
        "metadata": { "_total": total, "_count": total, "_page": 1 }
    });
    (
        [(header::SET_COOKIE, "__puus=rotated; Path=/; HttpOnly")],
        Json(body),
    )
        .into_response()
}

async fn mkdir(Json(body): Json<Value>) -> Response {
    let name = body
        .get("file_name")
        .and_then(Value::as_str)
        .unwrap_or("folder");
    ok(
        json!({"fid": format!("dir-{name}"), "finish": true}),
        json!({}),
    )
}

async fn rename(Json(body): Json<Value>) -> Response {
    assert!(body.get("fid").is_some());
    ok(json!({}), json!({}))
}

async fn move_files(Json(body): Json<Value>) -> Response {
    assert!(body.get("filelist").is_some());
    ok(json!({}), json!({}))
}

async fn delete_files(Json(body): Json<Value>) -> Response {
    assert!(body.get("filelist").is_some());
    ok(json!({}), json!({}))
}

async fn recycle_list(State(state): State<SharedState>) -> Response {
    let guard = state.lock().expect("fake state");
    let items = guard.recycle_items.clone();
    let count = items.len();
    ok(
        json!({ "list": items, "total": count }),
        json!({ "_count": count, "_page": 1, "_size": 50 }),
    )
}

async fn recycle_recover(State(state): State<SharedState>, Json(body): Json<Value>) -> Response {
    let count = recycle_action_records(&body);
    state.lock().expect("fake state").recycle_restore_calls += count;
    ok(
        json!({ "task_id": "restore-task", "finish": false }),
        json!({}),
    )
}

async fn recycle_remove(State(state): State<SharedState>, Json(body): Json<Value>) -> Response {
    let count = recycle_action_records(&body);
    state.lock().expect("fake state").recycle_remove_calls += count;
    ok(
        json!({ "task_id": "remove-task", "finish": false }),
        json!({}),
    )
}

fn recycle_action_records(body: &Value) -> usize {
    assert_eq!(body.get("select_mode").and_then(Value::as_i64), Some(2));
    body.get("record_list")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

async fn download_url(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let fid = body
        .get("fids")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .unwrap_or("file-1");
    let mut guard = state.lock().expect("fake state");
    guard.download_url_calls += 1;
    let total = guard.download_bytes.len();
    drop(guard);
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("127.0.0.1");
    ok(
        json!([{
            "fid": fid,
            "file_name": "hello.txt",
            "size": total,
            "download_url": format!("http://{host}/download/{fid}"),
            "md5": ""
        }]),
        json!({}),
    )
}

async fn upload_pre(State(state): State<SharedState>, Json(body): Json<Value>) -> Response {
    let mut guard = state.lock().expect("fake state");
    guard.pre_calls += 1;
    let name = body
        .get("file_name")
        .and_then(Value::as_str)
        .unwrap_or("upload.bin");
    ok(
        json!({
            "finish": false,
            "task_id": "task-1",
            "upload_id": "upload-1",
            "auth_info": "auth-info",
            "upload_url": "https://oss.example.com",
            "obj_key": format!("files/{name}"),
            "fid": "new-file-1",
            "bucket": "bucket",
            "format_type": "application/octet-stream",
            "callback": {"callbackUrl": "https://callback", "callbackBody": "{}"},
            "auth_info_expried": 0
        }),
        json!({"part_size": 8 * 1024 * 1024, "part_thread": 3}),
    )
}

async fn update_hash() -> Response {
    ok(json!({"finish": false}), json!({}))
}

async fn upload_auth(Json(body): Json<Value>) -> Response {
    assert!(body.get("auth_info").is_some());
    assert!(body.get("auth_meta").is_some());
    ok(json!({"auth_key": "auth-key"}), json!({}))
}

async fn upload_finish(State(state): State<SharedState>) -> Response {
    state.lock().expect("fake state").upload_finished = true;
    ok(json!({}), json!({}))
}

async fn upload_part(
    State(state): State<SharedState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Path(path): Path<String>,
    body: Bytes,
) -> Response {
    assert_eq!(
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("auth-key")
    );
    assert!(path.contains("files/"));
    assert_eq!(query.get("uploadId").map(String::as_str), Some("upload-1"));
    let mut guard = state.lock().expect("fake state");
    guard.upload_parts += 1;
    guard.upload_bytes.extend_from_slice(&body);
    let etag = format!(
        "\"etag-{}\"",
        query.get("partNumber").cloned().unwrap_or_default()
    );
    (
        StatusCode::OK,
        [(header::ETAG, HeaderValue::from_str(&etag).expect("etag"))],
    )
        .into_response()
}

async fn complete_upload(State(state): State<SharedState>, body: Bytes) -> Response {
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("CompleteMultipartUpload"));
    assert!(text.contains("etag-1"));
    state.lock().expect("fake state").upload_complete = true;
    StatusCode::OK.into_response()
}

async fn download(
    State(state): State<SharedState>,
    Path(_fid): Path<String>,
    headers: HeaderMap,
) -> Response {
    let (data, delay, expired) = {
        let mut guard = state.lock().expect("fake state");
        let expired = guard.expire_next_download;
        if expired {
            guard.expire_next_download = false;
        }
        (
            guard.download_bytes.clone(),
            guard.download_delay_ms,
            expired,
        )
    };
    if delay > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
    }
    if expired {
        return StatusCode::FORBIDDEN.into_response();
    }
    let total = data.len();
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_range);
    match range {
        Some((start, end)) => {
            let end = end.min(total.saturating_sub(1));
            let body = data[start.min(total)..=end].to_vec();
            (
                StatusCode::PARTIAL_CONTENT,
                [
                    (
                        header::CONTENT_RANGE,
                        HeaderValue::from_str(&format!("bytes {start}-{end}/{total}"))
                            .expect("range header"),
                    ),
                    (header::ACCEPT_RANGES, HeaderValue::from_static("bytes")),
                    (header::ETAG, HeaderValue::from_static("\"download-etag\"")),
                ],
                body,
            )
                .into_response()
        }
        None => (
            StatusCode::OK,
            [(
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&total.to_string()).expect("content length"),
            )],
            data,
        )
            .into_response(),
    }
}

fn parse_range(raw: &str) -> Option<(usize, usize)> {
    let value = raw.strip_prefix("bytes=")?;
    let (start, end) = value.split_once('-')?;
    let start = start.parse().ok()?;
    let end = if end.is_empty() {
        usize::MAX
    } else {
        end.parse().ok()?
    };
    Some((start, end))
}

fn ok(data: Value, metadata: Value) -> Response {
    Json(json!({
        "status": 200,
        "code": 0,
        "message": "ok",
        "data": data,
        "metadata": metadata
    }))
    .into_response()
}
