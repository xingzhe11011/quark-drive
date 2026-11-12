use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use quark_protocol::{ClientConfig, QuarkClient, Session};
use serde_json::{json, Value};

#[derive(Default)]
struct MockState {
    login_headers: Mutex<Option<HeaderMap>>,
    login_body: Mutex<Option<Value>>,
    list_headers: Mutex<Option<HeaderMap>>,
    list_query: Mutex<Option<HashMap<String, String>>>,
    list_requests: AtomicUsize,
    redirect_requests: AtomicUsize,
    login_response: Mutex<Option<Value>>,
    login_redirect: Mutex<bool>,
    list_redirect: Mutex<bool>,
}

struct MockSecretServer {
    client: QuarkClient,
    state: Arc<MockState>,
    task: tokio::task::JoinHandle<()>,
}

impl MockSecretServer {
    async fn start() -> Self {
        let state = Arc::new(MockState::default());
        let app = Router::new()
            .route("/1/clouddrive/secret/v2/login", post(login))
            .route("/1/clouddrive/file/sort", get(list))
            .route(
                "/redirect-target",
                get(|State(state): State<Arc<MockState>>| async move {
                    state.redirect_requests.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"status": 200, "code": 0, "data": {}}))
                }),
            )
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = QuarkClient::new(
            Session::from_cookie_header("__pus=test-session; __puus=fixture-puus").unwrap(),
            ClientConfig {
                api_base: format!("http://{address}/1/clouddrive"),
                ..ClientConfig::default()
            },
        )
        .unwrap();
        Self {
            client,
            state,
            task,
        }
    }
}

impl Drop for MockSecretServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn login(
    State(state): State<Arc<MockState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    *state.login_headers.lock().unwrap() = Some(headers);
    *state.login_body.lock().unwrap() = Some(body);
    if *state.login_redirect.lock().unwrap() {
        return Redirect::temporary("/redirect-target").into_response();
    }
    let response = state
        .login_response
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| {
            json!({
                "status": 200,
                "code": 0,
                "data": {"succ": true, "token": "test-private-token"}
            })
        });
    (StatusCode::OK, Json(response)).into_response()
}

async fn list(
    State(state): State<Arc<MockState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    state.list_requests.fetch_add(1, Ordering::SeqCst);
    *state.list_headers.lock().unwrap() = Some(headers);
    *state.list_query.lock().unwrap() = Some(query.clone());
    if *state.list_redirect.lock().unwrap() {
        return Redirect::temporary("/redirect-target").into_response();
    }
    Json(json!({
        "status": 200,
        "code": 0,
        "data": {"list": [{
            "fid": "private-file",
            "pdir_fid": query["pdir_fid"],
            "file_name": "synthetic.txt",
            "file": true,
            "size": 42
        }]},
        "metadata": {"_total": 11, "_count": 1}
    }))
    .into_response()
}

#[tokio::test]
async fn unlock_and_list_use_exact_contract_and_raw_auth_header() {
    let server = MockSecretServer::start().await;
    let secret = server
        .client
        .secret_unlock("test-only-privacy-password")
        .await
        .unwrap();

    let login_headers = server.state.login_headers.lock().unwrap().take().unwrap();
    assert_eq!(
        server.state.login_body.lock().unwrap().take().unwrap(),
        json!({"secret_pwd": "test-only-privacy-password"})
    );
    assert_eq!(login_headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(login_headers[header::ORIGIN], "https://pan.quark.cn");
    assert_eq!(login_headers[header::REFERER], "https://pan.quark.cn");
    assert!(login_headers[header::COOKIE]
        .to_str()
        .unwrap()
        .contains("__pus=test-session"));
    assert!(!login_headers.contains_key("X-U-Drive-Secret-Auth"));

    let page = secret.list_dir("2", 1, 100, "file_type:asc").await.unwrap();
    assert_eq!(page.page, 1);
    assert_eq!(page.size, 1);
    assert_eq!(page.total, 11);
    assert_eq!(page.entries[0].fid, "private-file");

    let list_headers = server.state.list_headers.lock().unwrap().take().unwrap();
    assert_eq!(list_headers["X-U-Drive-Secret-Auth"], "test-private-token");
    assert!(!list_headers.contains_key(header::AUTHORIZATION));
    let query = server.state.list_query.lock().unwrap().take().unwrap();
    assert_eq!(query["pdir_fid"], "2");
    assert_eq!(query["_page"], "1");
    assert_eq!(query["_size"], "100");
    assert_eq!(query["_fetch_total"], "1");
    assert_eq!(query["_fetch_sub_dirs"], "0");
    assert_eq!(query["name_space"], "0");
    assert_eq!(query["_sort"], "file_type:asc");
    assert_eq!(query["pr"], "ucpro");
    assert_eq!(query["fr"], "pc");
}

#[tokio::test]
async fn lock_is_shared_by_clones_and_prevents_further_requests() {
    let server = MockSecretServer::start().await;
    let secret = server
        .client
        .secret_unlock("test-only-privacy-password")
        .await
        .unwrap();
    let clone = secret.clone();
    secret.lock();
    assert!(clone.list_dir("2", 1, 20, "file_type:asc").await.is_err());
    assert_eq!(server.state.list_requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn strict_success_and_token_fields_are_required() {
    for data in [
        json!({}),
        json!({"succ": false, "token": "test-private-token"}),
        json!({"succ": "true", "token": "test-private-token"}),
        json!({"succ": 1, "token": "test-private-token"}),
        json!({"succ": true}),
        json!({"succ": true, "token": ""}),
        json!({"succ": true, "token": 1}),
        json!({"succ": true, "token": null}),
        json!([]),
        json!(true),
    ] {
        let server = MockSecretServer::start().await;
        *server.state.login_response.lock().unwrap() = Some(json!({
            "status": 200,
            "code": 0,
            "data": data
        }));
        let error = match server
            .client
            .secret_unlock("test-only-privacy-password")
            .await
        {
            Ok(_) => panic!("malformed login response was accepted"),
            Err(error) => error,
        };
        assert!(!error.to_string().contains("test-only-privacy-password"));
        assert!(!error.to_string().contains("test-private-token"));
    }
}

#[tokio::test]
async fn empty_password_is_rejected_without_a_request() {
    let server = MockSecretServer::start().await;
    assert!(server.client.secret_unlock("").await.is_err());
    assert!(server.state.login_headers.lock().unwrap().is_none());
}

#[tokio::test]
async fn private_redirects_are_not_followed() {
    let server = MockSecretServer::start().await;
    let secret = server
        .client
        .secret_unlock("test-only-privacy-password")
        .await
        .unwrap();
    *server.state.list_redirect.lock().unwrap() = true;
    let error = secret
        .list_dir("2", 1, 20, "file_type:asc")
        .await
        .unwrap_err();
    assert!(!error.to_string().contains("test-private-token"));
    assert_eq!(server.state.redirect_requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn private_login_redirects_are_not_followed() {
    let server = MockSecretServer::start().await;
    *server.state.login_redirect.lock().unwrap() = true;
    let error = match server
        .client
        .secret_unlock("test-only-privacy-password")
        .await
    {
        Ok(_) => panic!("private login redirect was accepted"),
        Err(error) => error,
    };
    assert!(!error.to_string().contains("test-only-privacy-password"));
    assert_eq!(server.state.redirect_requests.load(Ordering::SeqCst), 0);
}
