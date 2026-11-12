use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use quark_protocol::{ClientConfig, QuarkClient, Session};
use serde_json::{json, Value};

#[derive(Default)]
struct TransferState {
    move_headers: Option<HeaderMap>,
    move_query: Option<HashMap<String, String>>,
    move_body: Option<Value>,
    download_headers: Option<HeaderMap>,
    download_body: Option<Value>,
    cdn_headers: Option<HeaderMap>,
    upload_pre_headers: Option<HeaderMap>,
    upload_pre_body: Option<Value>,
    upload_auth_headers: Option<HeaderMap>,
    upload_auth_body: Option<Value>,
    part_headers: Option<HeaderMap>,
    part_body: Option<Vec<u8>>,
    complete_headers: Option<HeaderMap>,
    finish_headers: Option<HeaderMap>,
}

struct MockServer {
    base_url: String,
    state: Arc<Mutex<TransferState>>,
    task: tokio::task::JoinHandle<()>,
}

impl MockServer {
    async fn start() -> Self {
        let state = Arc::new(Mutex::new(TransferState::default()));
        let app = Router::new()
            .route("/1/clouddrive/secret/v2/login", post(login))
            .route("/1/clouddrive/secret/file/move", post(move_files))
            .route("/1/clouddrive/file/download", post(download_urls))
            .route("/1/clouddrive/file/upload/pre", post(upload_pre))
            .route("/1/clouddrive/file/update/hash", post(update_hash))
            .route("/1/clouddrive/file/upload/auth", post(upload_auth))
            .route("/1/clouddrive/file/upload/finish", post(upload_finish))
            .route("/cdn/file", get(cdn_file))
            .route("/oss/{*path}", put(upload_part).post(complete_upload))
            .with_state(Arc::clone(&state));
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

    fn config(&self) -> ClientConfig {
        ClientConfig {
            api_base: format!("{}/1/clouddrive", self.base_url),
            oss_base_override: Some(format!("{}/oss", self.base_url)),
            ..ClientConfig::default()
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn login() -> Json<Value> {
    Json(json!({
        "status": 200,
        "code": 0,
        "data": {"succ": true, "token": "private-token"}
    }))
}

async fn move_files(
    State(state): State<Arc<Mutex<TransferState>>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    let mut state = state.lock().unwrap();
    state.move_headers = Some(headers);
    state.move_query = Some(query);
    state.move_body = Some(body);
    Json(json!({"status": 200, "code": 0, "data": {}}))
}

async fn download_urls(
    State(state): State<Arc<Mutex<TransferState>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    state.lock().unwrap().download_headers = Some(headers);
    state.lock().unwrap().download_body = Some(body);
    Json(json!({
        "status": 200,
        "code": 0,
        "data": [{
            "fid": "private-file",
            "file_name": "private.bin",
            "size": 4,
            "download_url": "/cdn/file",
            "md5": ""
        }]
    }))
}

async fn cdn_file(
    State(state): State<Arc<Mutex<TransferState>>>,
    headers: HeaderMap,
) -> axum::response::Response {
    state.lock().unwrap().cdn_headers = Some(headers);
    (
        StatusCode::PARTIAL_CONTENT,
        [(header::CONTENT_RANGE, "bytes 0-3/4")],
        [1_u8, 2, 3, 4],
    )
        .into_response()
}

async fn upload_pre(
    State(state): State<Arc<Mutex<TransferState>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    let mut state = state.lock().unwrap();
    state.upload_pre_headers = Some(headers);
    state.upload_pre_body = Some(body);
    Json(json!({
        "status": 200,
        "code": 0,
        "data": {
            "finish": false,
            "task_id": "task-id",
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

async fn upload_auth(
    State(state): State<Arc<Mutex<TransferState>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    let mut state = state.lock().unwrap();
    state.upload_auth_headers = Some(headers);
    state.upload_auth_body = Some(body);
    Json(json!({"status": 200, "code": 0, "data": {"auth_key": "oss-auth-key"}}))
}

async fn upload_part(
    State(state): State<Arc<Mutex<TransferState>>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let mut state = state.lock().unwrap();
    state.part_headers = Some(headers);
    state.part_body = Some(body.to_vec());
    (StatusCode::OK, [(header::ETAG, "\"etag-1\"")]).into_response()
}

async fn complete_upload(
    State(state): State<Arc<Mutex<TransferState>>>,
    headers: HeaderMap,
) -> StatusCode {
    state.lock().unwrap().complete_headers = Some(headers);
    StatusCode::OK
}

async fn upload_finish(
    State(state): State<Arc<Mutex<TransferState>>>,
    headers: HeaderMap,
) -> Json<Value> {
    state.lock().unwrap().finish_headers = Some(headers);
    Json(json!({"status": 200, "code": 0, "data": {}}))
}

#[tokio::test]
async fn private_transfer_contract_keeps_token_on_api_and_off_cdn() {
    let server = MockServer::start().await;
    let client = QuarkClient::new(
        Session::from_cookie_header("__pus=test-session").unwrap(),
        server.config(),
    )
    .unwrap();
    let secret = client
        .secret_unlock("test-only-private-password")
        .await
        .unwrap();

    secret
        .move_files(
            &["private-file".to_string(), "private-folder".to_string()],
            "private-destination",
        )
        .await
        .unwrap();
    {
        let state = server.state.lock().unwrap();
        let headers = state.move_headers.as_ref().unwrap();
        assert_eq!(headers["X-U-Drive-Secret-Auth"], "private-token");
        assert_eq!(state.move_query.as_ref().unwrap()["pr"], "ucpro");
        assert_eq!(state.move_query.as_ref().unwrap()["fr"], "pc");
        assert_eq!(
            state.move_body.as_ref().unwrap(),
            &json!({
                "action_type": 1,
                "exclude_fids": [],
                "filelist": ["private-file", "private-folder"],
                "to_pdir_fid": "private-destination"
            })
        );
    }

    let urls = secret
        .download_urls(&["private-file".to_string()])
        .await
        .unwrap();
    assert_eq!(urls[0].download_url, "/cdn/file");
    let response = secret
        .download_range(&format!("{}/cdn/file", server.base_url), 0, Some(3))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.bytes().await.unwrap().as_ref(), [1_u8, 2, 3, 4]);
    {
        let state = server.state.lock().unwrap();
        assert_eq!(
            state.download_headers.as_ref().unwrap()["X-U-Drive-Secret-Auth"],
            "private-token"
        );
        assert_eq!(
            state.download_body.as_ref().unwrap(),
            &json!({"fids": ["private-file"]})
        );
        let cdn_headers = state.cdn_headers.as_ref().unwrap();
        assert!(!cdn_headers.contains_key("X-U-Drive-Secret-Auth"));
        assert_eq!(cdn_headers[header::RANGE], "bytes=0-3");
    }

    let pre = secret
        .upload_pre(
            "private.bin",
            4,
            "private-parent",
            "application/octet-stream",
        )
        .await
        .unwrap();
    assert!(!pre.data.finish);
    let hit = secret
        .update_hash("md5", "sha1", &pre.data.task_id)
        .await
        .unwrap();
    assert!(!hit);
    let time = httpdate::fmt_http_date(std::time::SystemTime::now());
    let etag = secret
        .upload_part(
            &pre,
            1,
            "application/octet-stream",
            &time,
            bytes::Bytes::from_static(&[1, 2, 3, 4]),
        )
        .await
        .unwrap();
    secret.complete_multipart(&pre, &[etag]).await.unwrap();
    secret
        .upload_finish(&pre.data.obj_key, &pre.data.task_id)
        .await
        .unwrap();

    let state = server.state.lock().unwrap();
    assert_eq!(
        state.upload_pre_headers.as_ref().unwrap()["X-U-Drive-Secret-Auth"],
        "private-token"
    );
    assert_eq!(
        state.upload_auth_headers.as_ref().unwrap()["X-U-Drive-Secret-Auth"],
        "private-token"
    );
    assert_eq!(
        state.part_headers.as_ref().unwrap()[header::AUTHORIZATION],
        "oss-auth-key"
    );
    assert!(!state
        .part_headers
        .as_ref()
        .unwrap()
        .contains_key("X-U-Drive-Secret-Auth"));
    assert!(!state
        .complete_headers
        .as_ref()
        .unwrap()
        .contains_key("X-U-Drive-Secret-Auth"));
    assert_eq!(
        state.finish_headers.as_ref().unwrap()["X-U-Drive-Secret-Auth"],
        "private-token"
    );
    assert_eq!(state.part_body.as_deref(), Some([1_u8, 2, 3, 4].as_slice()));
}

use axum::response::IntoResponse;
