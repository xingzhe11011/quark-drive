use std::collections::HashMap;

use axum::extract::Query;
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use quark_protocol::{ClientConfig, Error, QuarkClient, Session};
use serde_json::{json, Value};

struct MockRegistrationServer {
    client: QuarkClient,
    task: tokio::task::JoinHandle<()>,
}

impl MockRegistrationServer {
    async fn start(status: StatusCode, response: String) -> Self {
        let app = Router::new().route(
            "/1/clouddrive/secret/check/register",
            post(
                move |Query(query): Query<HashMap<String, String>>,
                      headers: HeaderMap,
                      Json(body): Json<Value>| {
                    let response = response.clone();
                    async move {
                        assert_eq!(
                            query,
                            HashMap::from([
                                ("pr".to_string(), "ucpro".to_string()),
                                ("fr".to_string(), "pc".to_string()),
                            ])
                        );
                        assert_eq!(body, json!({}));
                        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
                        assert_eq!(headers[header::ORIGIN], "https://pan.quark.cn");
                        assert_eq!(headers[header::REFERER], "https://pan.quark.cn");
                        assert!(headers[header::COOKIE]
                            .to_str()
                            .unwrap()
                            .contains("__pus=test-session"));
                        assert!(!headers.contains_key("X-U-Drive-Secret-Auth"));
                        (
                            status,
                            [(header::CONTENT_TYPE, "application/json")],
                            response,
                        )
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = QuarkClient::new(
            Session::from_cookie_header("__pus=test-session").unwrap(),
            ClientConfig {
                api_base: format!("http://{address}/1/clouddrive"),
                ..ClientConfig::default()
            },
        )
        .unwrap();
        Self { client, task }
    }
}

impl Drop for MockRegistrationServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn secret_registration_posts_empty_json_and_parses_boolean() {
    for has_register in [true, false] {
        let server = MockRegistrationServer::start(
            StatusCode::OK,
            json!({"status": 200, "code": 0, "data": {"has_register": has_register}}).to_string(),
        )
        .await;
        assert_eq!(
            server.client.secret_is_registered().await.unwrap(),
            has_register
        );
    }
}

#[tokio::test]
async fn secret_registration_rejects_missing_and_non_boolean_flags() {
    for data in [
        json!({}),
        json!({"has_register": null}),
        json!({"has_register": "true"}),
        json!({"has_register": 1}),
        json!({"has_register": []}),
        json!({"has_register": {}}),
        json!([]),
        json!([true]),
        json!(true),
        json!("true"),
    ] {
        let server = MockRegistrationServer::start(
            StatusCode::OK,
            json!({"status": 200, "code": 0, "data": data}).to_string(),
        )
        .await;
        let error = server.client.secret_is_registered().await.unwrap_err();
        assert!(matches!(error, Error::Other(_)), "{error:?}");
        assert!(!error.is_auth());
    }
}

#[tokio::test]
async fn secret_registration_rejects_missing_data_and_invalid_json() {
    for response in [
        json!({"status": 200, "code": 0}).to_string(),
        json!({"status": 200, "code": 0, "data": null}).to_string(),
        "{invalid json".to_string(),
    ] {
        let server = MockRegistrationServer::start(StatusCode::OK, response).await;
        let error = server.client.secret_is_registered().await.unwrap_err();
        assert!(!error.is_auth());
        assert!(!error.retryable());
    }
}

#[tokio::test]
async fn secret_registration_preserves_api_error_classification() {
    for (status, code, message, is_auth) in [
        (
            StatusCode::BAD_REQUEST,
            14001,
            "Bad Parameter: [param missing!]",
            false,
        ),
        (
            StatusCode::FORBIDDEN,
            24005,
            "secret space limit: get file vo",
            false,
        ),
        (StatusCode::UNAUTHORIZED, 31001, "login required", true),
        (StatusCode::FORBIDDEN, 31001, "login expired", true),
    ] {
        let server = MockRegistrationServer::start(
            status,
            json!({"status": status.as_u16(), "code": code, "message": message}).to_string(),
        )
        .await;
        let error = server.client.secret_is_registered().await.unwrap_err();
        assert_eq!(error.is_auth(), is_auth, "{error:?}");
    }
}
