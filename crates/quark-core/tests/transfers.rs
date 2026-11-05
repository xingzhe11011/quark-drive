use std::time::{Duration, Instant};

use quark_core::{AppCore, CoreConfig, DownloadItem};
use quark_protocol::QrLoginConfig;
use quark_store::Store;
use quark_testkit::FakeQuarkServer;
use tempfile::TempDir;

fn config_for(server: &FakeQuarkServer, temp: &TempDir) -> CoreConfig {
    CoreConfig {
        data_dir: temp.path().join("data"),
        download_dir: temp.path().join("downloads"),
        api_base: server.api_base(),
        pc_api_base: server.api_base(),
        account_base: Some(server.base_url.clone()),
        oss_base_override: Some(server.oss_base()),
        qr_login: QrLoginConfig {
            token_url: server.qr_token_url(),
            poll_url: server.qr_poll_url(),
            account_info_url: server.qr_account_info_url(),
            scan_url: server.qr_scan_url(),
            ..QrLoginConfig::default()
        },
        ..CoreConfig::default()
    }
}

async fn setup(server: &FakeQuarkServer, temp: &TempDir) -> (AppCore, String) {
    let core = AppCore::new(config_for(server, temp)).await.unwrap();
    let account = core
        .login_with_cookie("__puus=old; __pus=pus; __uid=test")
        .await
        .unwrap();
    (core, account.id)
}

#[tokio::test]
async fn upload_and_download_end_to_end() {
    let server = FakeQuarkServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, account_id) = setup(&server, &temp).await;

    let page = core.list_dir(&account_id, "0", 1, 100).await.unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.entries[1].name, "hello.txt");

    let source = temp.path().join("sample.bin");
    let payload = (0..1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    tokio::fs::write(&source, &payload).await.unwrap();
    let ids = core
        .enqueue_upload(&account_id, vec![source.to_string_lossy().to_string()], "0")
        .await
        .unwrap();
    assert_eq!(ids.len(), 1);
    wait_for(&core, &ids[0], "done").await;
    {
        let state = server.state.lock().unwrap();
        assert_eq!(state.upload_bytes, payload);
        assert!(state.upload_complete);
        assert!(state.upload_finished);
    }

    let file_count = server.state.lock().unwrap().download_bytes.len();
    let download_ids = core
        .enqueue_download(
            &account_id,
            vec![DownloadItem {
                fid: "file-1".to_string(),
                name: "hello.bin".to_string(),
                size: file_count as i64,
                is_dir: false,
            }],
            Some(temp.path().join("downloads").to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    assert_eq!(download_ids.len(), 1);
    wait_for(&core, &download_ids[0], "done").await;

    let transfer = core
        .list_transfers(20)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == download_ids[0])
        .unwrap();
    let downloaded = tokio::fs::read(&transfer.local_path).await.unwrap();
    assert_eq!(downloaded, server.state.lock().unwrap().download_bytes);

    let recovered = core.list_transfers(20).await.unwrap();
    assert_eq!(recovered.len(), 2);
    assert!(recovered.iter().all(|item| item.status == "done"));
}

#[tokio::test]
async fn refreshes_expired_download_link() {
    let server = FakeQuarkServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, account_id) = setup(&server, &temp).await;
    server.state.lock().unwrap().expire_next_download = true;
    let total = server.state.lock().unwrap().download_bytes.len();

    let ids = core
        .enqueue_download(
            &account_id,
            vec![DownloadItem {
                fid: "file-1".to_string(),
                name: "refresh.bin".to_string(),
                size: total as i64,
                is_dir: false,
            }],
            Some(temp.path().join("refresh").to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    wait_for(&core, &ids[0], "done").await;

    let transfer = core
        .list_transfers(20)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == ids[0])
        .unwrap();
    let downloaded = tokio::fs::read(&transfer.local_path).await.unwrap();
    assert_eq!(downloaded, server.state.lock().unwrap().download_bytes);
    assert_eq!(server.state.lock().unwrap().download_url_calls, 2);
}

#[tokio::test]
async fn pause_then_resume_continues_download() {
    let server = FakeQuarkServer::start().await;
    let temp = TempDir::new().unwrap();
    {
        let mut state = server.state.lock().unwrap();
        state.download_bytes = (0..64 * 1024 * 1024)
            .map(|index| (index % 251) as u8)
            .collect();
        state.download_delay_ms = 600;
    }
    let (core, account_id) = setup(&server, &temp).await;
    let total = server.state.lock().unwrap().download_bytes.len();

    let ids = core
        .enqueue_download(
            &account_id,
            vec![DownloadItem {
                fid: "file-1".to_string(),
                name: "resume.bin".to_string(),
                size: total as i64,
                is_dir: false,
            }],
            Some(temp.path().join("resume").to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    wait_for_progress(&core, &ids[0]).await;
    core.pause_transfer(&ids[0]).await.unwrap();
    wait_for(&core, &ids[0], "paused").await;

    let paused = core
        .list_transfers(20)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == ids[0])
        .unwrap();
    assert!(paused.done_bytes > 0);
    assert!(paused.done_bytes < total as i64);

    server.state.lock().unwrap().download_delay_ms = 0;
    core.resume_transfer(&ids[0]).await.unwrap();
    wait_for(&core, &ids[0], "done").await;
    let transfer = core
        .list_transfers(20)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == ids[0])
        .unwrap();
    let downloaded = tokio::fs::read(&transfer.local_path).await.unwrap();
    assert_eq!(downloaded, server.state.lock().unwrap().download_bytes);
}

#[tokio::test]
async fn restarts_and_recovers_partial_download() {
    let server = FakeQuarkServer::start().await;
    let temp = TempDir::new().unwrap();
    {
        let mut state = server.state.lock().unwrap();
        state.download_bytes = (0..25 * 1024 * 1024)
            .map(|index| (index % 251) as u8)
            .collect();
        state.download_delay_ms = 400;
    }
    let total = server.state.lock().unwrap().download_bytes.len();
    let (core, account_id) = setup(&server, &temp).await;
    let ids = core
        .enqueue_download(
            &account_id,
            vec![DownloadItem {
                fid: "file-1".to_string(),
                name: "recover.bin".to_string(),
                size: total as i64,
                is_dir: false,
            }],
            Some(temp.path().join("recover").to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    wait_for_progress(&core, &ids[0]).await;
    core.pause_transfer(&ids[0]).await.unwrap();
    wait_for(&core, &ids[0], "paused").await;
    let paused = core
        .list_transfers(20)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == ids[0])
        .unwrap();
    assert!(paused.done_bytes > 0);
    assert!(paused.done_bytes < total as i64);
    drop(core);

    let store = Store::connect(temp.path().join("data/quark-drive.db"))
        .await
        .unwrap();
    store
        .patch_transfer_state(
            &ids[0],
            "running",
            "downloading",
            paused.done_bytes,
            None,
            None,
        )
        .await
        .unwrap();
    drop(store);

    server.state.lock().unwrap().download_delay_ms = 0;
    let recovered = AppCore::new(config_for(&server, &temp)).await.unwrap();
    wait_for(&recovered, &ids[0], "done").await;
    let transfer = recovered
        .list_transfers(20)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == ids[0])
        .unwrap();
    let downloaded = tokio::fs::read(&transfer.local_path).await.unwrap();
    assert_eq!(downloaded, server.state.lock().unwrap().download_bytes);
}

#[tokio::test]
async fn folder_download_keeps_nested_and_empty_directories() {
    let server = FakeQuarkServer::start().await;
    let temp = TempDir::new().unwrap();
    let (core, account_id) = setup(&server, &temp).await;
    let destination = temp.path().join("folder");
    let ids = core
        .enqueue_download(
            &account_id,
            vec![DownloadItem {
                fid: "dir-1".to_string(),
                name: "文档".to_string(),
                size: 0,
                is_dir: true,
            }],
            Some(destination.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    assert_eq!(ids.len(), 1);
    wait_for(&core, &ids[0], "done").await;

    let transfer = core
        .list_transfers(20)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == ids[0])
        .unwrap();
    assert!(transfer.local_path.ends_with("文档/nested.txt"));
    let downloaded = tokio::fs::read(&transfer.local_path).await.unwrap();
    let expected = server.state.lock().unwrap().download_bytes[..11].to_vec();
    assert_eq!(downloaded, expected);
    assert!(destination.join("文档/空目录").is_dir());
}

#[tokio::test]
async fn qr_login_creates_account_and_lists_files() {
    let server = FakeQuarkServer::start().await;
    let temp = TempDir::new().unwrap();
    let core = AppCore::new(config_for(&server, &temp)).await.unwrap();

    let started = core.start_qr_login().await.unwrap();
    assert!(started.qr_url.contains("token="));
    assert!(started.expires_at > 0);

    let waiting = core.poll_qr_login(&started.session_id).await.unwrap();
    assert_eq!(waiting.state, "waiting");

    server.state.lock().unwrap().qr_ticket_ready = true;
    let success = core.poll_qr_login(&started.session_id).await.unwrap();
    assert_eq!(success.state, "success");
    let account = success.account.expect("login should return an account");
    assert_eq!(account.nickname, "测试用户");
    assert_eq!(account.uid, "test-uid");
    assert_eq!(account.avatar_url, "https://image.quark.cn/avatar/test.png");
    assert_eq!(account.vip_expire_at, Some(1_812_470_400_000));

    let page = core.list_dir(&account.id, "0", 1, 100).await.unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.entries[1].name, "hello.txt");

    let recycle = core.list_recycle(&account.id, 1, 50).await.unwrap();
    assert_eq!(recycle.total, 2);
    assert_eq!(recycle.entries[0].name, "旧文件夹");
    assert!(recycle.entries[0].is_dir);
    core.restore_recycle(&account.id, &["record-1".to_string()])
        .await
        .unwrap();
    core.delete_recycle(&account.id, &["record-2".to_string()])
        .await
        .unwrap();

    assert!(core.poll_qr_login(&started.session_id).await.is_err());
}

#[tokio::test]
async fn restores_saved_session_after_restart() {
    let server = FakeQuarkServer::start().await;
    let temp = TempDir::new().unwrap();
    let core = AppCore::new(config_for(&server, &temp)).await.unwrap();
    let account = core
        .login_with_cookie("__puus=old; __pus=pus; __uid=test")
        .await
        .unwrap();
    drop(core);

    let restarted = AppCore::new(config_for(&server, &temp)).await.unwrap();
    let accounts = restarted.accounts().await.unwrap();
    assert!(accounts.iter().any(|item| item.id == account.id));

    let refreshed = restarted.refresh_account(&account.id).await.unwrap();
    assert_eq!(refreshed.nickname, "测试用户");
    assert_eq!(refreshed.vip_expire_at, Some(1_812_470_400_000));
}

async fn wait_for_progress(core: &AppCore, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(item) = core
            .list_transfers(50)
            .await
            .unwrap()
            .into_iter()
            .find(|item| item.id == id)
        {
            if item.done_bytes > 0 {
                return;
            }
            assert!(
                !matches!(item.status.as_str(), "done" | "failed" | "canceled"),
                "transfer ended as {} before it could be paused",
                item.status
            );
        }
        if Instant::now() > deadline {
            panic!("timed out waiting for transfer {id} to make progress");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn wait_for(core: &AppCore, id: &str, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(item) = core
            .list_transfers(50)
            .await
            .unwrap()
            .into_iter()
            .find(|item| item.id == id)
        {
            if item.status == expected {
                return;
            }
            if item.status == "failed" || item.status == "canceled" {
                panic!(
                    "transfer ended as {}: {:?}",
                    item.status, item.error_message
                );
            }
        }
        if Instant::now() > deadline {
            panic!("timed out waiting for transfer {id} to be {expected}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
