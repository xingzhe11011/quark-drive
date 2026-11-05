use std::time::SystemTime;

use bytes::Bytes;
use quark_protocol::{
    ClientConfig, QrLogin, QrLoginConfig, QrLoginPoll, QuarkClient, Session, DEFAULT_SORT,
};
use quark_testkit::FakeQuarkServer;
use url::Url;

fn client_for(server: &FakeQuarkServer) -> QuarkClient {
    let session = Session::from_cookie_header("__puus=old; __pus=pus; __uid=test").unwrap();
    let config = ClientConfig {
        api_base: server.api_base(),
        pc_api_base: server.api_base(),
        account_base: server.base_url.clone(),
        oss_base_override: Some(server.oss_base()),
        ..ClientConfig::default()
    };
    QuarkClient::new(session, config).unwrap()
}

#[tokio::test]
async fn complete_protocol_flow() {
    let server = FakeQuarkServer::start().await;
    let client = client_for(&server);

    let member = client.member_info().await.unwrap();
    assert_eq!(member.uid, "test-uid");
    assert_eq!(member.nickname, "测试用户");
    assert_eq!(member.expire_at, Some(1_812_470_400_000));
    assert_eq!(member.total_capacity, 1024 * 1024 * 1024);
    assert!(member.is_vip());

    let page = client.list_dir("0", 1, 100, DEFAULT_SORT).await.unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.entries[1].file_name, "hello.txt");
    assert!(!page.entries[1].is_dir());
    assert!(page.entries[0].is_dir());

    let recycle = client.list_recycle(1, 50).await.unwrap();
    assert_eq!(recycle.total, 2);
    assert_eq!(recycle.entries[0].file_name, "旧文件夹");
    assert!(recycle.entries[0].is_dir());
    assert!(!recycle.entries[1].is_dir());
    client
        .restore_recycle(&["record-1".to_string()])
        .await
        .unwrap();
    client
        .delete_recycle(&["record-2".to_string()])
        .await
        .unwrap();

    // The API rotates __puus on responses; the client must persist it.
    assert_eq!(client.session().get("__puus"), Some("rotated"));

    let urls = client.download_urls(&["file-1".to_string()]).await.unwrap();
    assert_eq!(urls.len(), 1);
    let response = client
        .download_range(&urls[0].download_url, 0, Some(4))
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::PARTIAL_CONTENT);
    let body = response.bytes().await.unwrap();
    assert_eq!(&body[..], &[0, 1, 2, 3, 4]);

    let pre = client
        .upload_pre("hello.txt", 11, "0", "text/plain")
        .await
        .unwrap();
    assert!(!pre.data.finish);
    assert_eq!(pre.metadata.part_size, 8 * 1024 * 1024);
    assert!(!client
        .update_hash("md5", "sha1", &pre.data.task_id)
        .await
        .unwrap());
    let time_str = httpdate::fmt_http_date(SystemTime::now());
    let etag = client
        .upload_part(
            &pre,
            1,
            "text/plain",
            &time_str,
            Bytes::from_static(b"hello world"),
        )
        .await
        .unwrap();
    assert_eq!(etag, "\"etag-1\"");
    client.complete_multipart(&pre, &[etag]).await.unwrap();
    client
        .upload_finish(&pre.data.obj_key, &pre.data.task_id)
        .await
        .unwrap();

    let state = server.state.lock().unwrap();
    assert_eq!(state.pre_calls, 1);
    assert_eq!(state.upload_parts, 1);
    assert_eq!(state.upload_bytes, b"hello world");
    assert!(state.upload_complete);
    assert!(state.upload_finished);
    assert_eq!(state.recycle_restore_calls, 1);
    assert_eq!(state.recycle_remove_calls, 1);
}

#[tokio::test]
async fn retryable_errors_are_marked() {
    let server = FakeQuarkServer::start().await;
    let client = client_for(&server);
    server.state.lock().unwrap().fail_next_list = true;
    let error = client
        .list_dir("0", 1, 100, DEFAULT_SORT)
        .await
        .expect_err("429 should fail");
    assert!(error.retryable());
    let page = client.list_dir("0", 1, 100, DEFAULT_SORT).await.unwrap();
    assert_eq!(page.total, 2);
}

#[tokio::test]
async fn qr_login_shares_cookies_and_lists_files() {
    let server = FakeQuarkServer::start().await;
    let config = QrLoginConfig {
        token_url: server.qr_token_url(),
        poll_url: server.qr_poll_url(),
        account_info_url: server.qr_account_info_url(),
        scan_url: server.qr_scan_url(),
        ..QrLoginConfig::default()
    };
    let login = QrLogin::start(config).await.unwrap();
    let qr_url = Url::parse(login.qr_url()).unwrap();
    assert_eq!(qr_url.path(), "/qr/scan");
    let query = qr_url
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        query.get("token").map(|value| value.as_ref()),
        Some("fake-qr-token")
    );
    assert_eq!(
        query.get("client_id").map(|value| value.as_ref()),
        Some("532")
    );
    assert_eq!(
        query.get("ssb").map(|value| value.as_ref()),
        Some("weblogin")
    );

    assert!(matches!(login.poll().await.unwrap(), QrLoginPoll::Waiting));

    server.state.lock().unwrap().qr_ticket_ready = true;
    let session = match login.poll().await.unwrap() {
        QrLoginPoll::Success(session) => session,
        other => panic!("expected QR login success, got {other:?}"),
    };
    assert_eq!(session.get("__pus"), Some("fake-pus"));
    assert_eq!(session.get("__puus"), None);

    let client = QuarkClient::new(
        session,
        ClientConfig {
            api_base: server.api_base(),
            pc_api_base: server.api_base(),
            account_base: server.base_url.clone(),
            oss_base_override: Some(server.oss_base()),
            ..ClientConfig::default()
        },
    )
    .unwrap();
    let page = client.list_dir("0", 1, 100, DEFAULT_SORT).await.unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(client.session().get("__puus"), Some("rotated"));

    let state = server.state.lock().unwrap();
    assert_eq!(state.qr_token_calls, 1);
    assert_eq!(state.qr_poll_calls, 2);
    assert_eq!(state.qr_account_info_calls, 1);
}

#[tokio::test]
async fn qr_login_reports_expired_code() {
    let server = FakeQuarkServer::start().await;
    let config = QrLoginConfig {
        token_url: server.qr_token_url(),
        poll_url: server.qr_poll_url(),
        account_info_url: server.qr_account_info_url(),
        scan_url: server.qr_scan_url(),
        ..QrLoginConfig::default()
    };
    let login = QrLogin::start(config).await.unwrap();
    server.state.lock().unwrap().qr_expired = true;
    assert!(matches!(login.poll().await.unwrap(), QrLoginPoll::Expired));
}
