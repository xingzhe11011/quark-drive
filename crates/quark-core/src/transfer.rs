use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use futures_util::stream::{self, StreamExt};
use quark_protocol::{QuarkClient, UploadPre};
use quark_store::TransferPartRow;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::{AppCore, CoreError, Result};

const MIN_PART_SIZE: i64 = 1024 * 1024;
const DEFAULT_PART_SIZE: i64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UploadSessionState {
    pre: UploadPre,
    part_size: i64,
    md5: String,
    sha1: String,
    #[serde(default)]
    hash_checked: bool,
}

pub(crate) async fn run_transfer(
    core: AppCore,
    id: String,
    cancel: CancellationToken,
) -> Result<()> {
    check_cancel(&cancel)?;
    let row = core
        .inner
        .store
        .get_transfer(&id)
        .await?
        .ok_or_else(|| CoreError::TransferNotFound(id.clone()))?;
    match row.kind.as_str() {
        "upload" => run_upload(&core, &row, &cancel).await,
        "download" => run_download(&core, &row, &cancel).await,
        other => Err(CoreError::Other(format!("未知任务类型: {other}"))),
    }
}

async fn run_upload(
    core: &AppCore,
    row: &quark_store::TransferRow,
    cancel: &CancellationToken,
) -> Result<()> {
    check_cancel(cancel)?;
    let client = core.client_for(&row.account_id).await?;
    let local_path = PathBuf::from(&row.local_path);
    if !local_path.exists() {
        return Err(CoreError::InvalidInput(format!(
            "本地文件不存在: {}",
            local_path.display()
        )));
    }
    let total = local_path.metadata()?.len() as i64;
    let mime = mime_guess::from_path(&local_path)
        .first_raw()
        .unwrap_or("application/octet-stream")
        .to_string();

    let mut state = row
        .session_json
        .as_deref()
        .and_then(|text| serde_json::from_str::<UploadSessionState>(text).ok());

    if state.is_none() {
        check_cancel(cancel)?;
        core.update_progress(&row.id, "running", "hashing", 0, total)
            .await?;
        let (md5, sha1) = hash_file(&local_path, cancel).await?;
        check_cancel(cancel)?;
        core.update_progress(&row.id, "running", "preparing", 0, total)
            .await?;
        let pre = client
            .upload_pre(&row.remote_name, total, &row.remote_parent_fid, &mime)
            .await?;
        core.persist_session(&row.account_id, &client).await?;
        if pre.data.finish {
            info!(transfer = %row.id, "rapid upload hit before parts");
            core.update_progress(&row.id, "done", "done", total, total)
                .await?;
            return Ok(());
        }
        let part_size = if pre.metadata.part_size >= MIN_PART_SIZE {
            pre.metadata.part_size
        } else {
            DEFAULT_PART_SIZE
        };
        let new_state = UploadSessionState {
            pre,
            part_size,
            md5,
            sha1,
            hash_checked: false,
        };
        save_upload_state(core, &row.id, &new_state).await?;
        state = Some(new_state);
    }

    let mut state = state.expect("upload state initialized");
    if !state.hash_checked {
        check_cancel(cancel)?;
        let hit = client
            .update_hash(&state.md5, &state.sha1, &state.pre.data.task_id)
            .await?;
        state.hash_checked = true;
        save_upload_state(core, &row.id, &state).await?;
        core.persist_session(&row.account_id, &client).await?;
        if hit {
            info!(transfer = %row.id, "rapid upload hit after hash");
            core.update_progress(&row.id, "done", "done", total, total)
                .await?;
            return Ok(());
        }
    }

    let part_size = state.part_size.max(MIN_PART_SIZE);
    let part_count = if total == 0 {
        0
    } else {
        (total + part_size - 1) / part_size
    };
    ensure_upload_parts(core, &row.id, total, part_size, part_count).await?;

    let mut done = parts_done_bytes(core, &row.id).await?;
    core.update_progress(&row.id, "running", "uploading", done, total)
        .await?;

    let parts = core.inner.store.list_parts(&row.id).await?;
    for part in parts.iter().filter(|part| part.status != "done") {
        check_cancel(cancel)?;
        let offset = part.offset as u64;
        let size = part.size as u64;
        let bytes = read_part(&local_path, offset, size).await?;
        let part_number = part.part_number as u32;
        let mut attempt = 0;
        let mut last_error = None;
        while attempt < 4 {
            attempt += 1;
            let time_str = httpdate::fmt_http_date(SystemTime::now());
            match client
                .upload_part(&state.pre, part_number, &mime, &time_str, bytes.clone())
                .await
            {
                Ok(etag) => {
                    let updated = TransferPartRow {
                        transfer_id: row.id.clone(),
                        part_number: part.part_number,
                        offset: part.offset,
                        size: part.size,
                        etag: Some(etag),
                        status: "done".to_string(),
                    };
                    core.inner.store.upsert_part(&updated).await?;
                    done += part.size;
                    core.update_progress(&row.id, "running", "uploading", done, total)
                        .await?;
                    core.persist_session(&row.account_id, &client).await?;
                    last_error = None;
                    break;
                }
                Err(err) => {
                    warn!(
                        transfer = %row.id,
                        part = part_number,
                        attempt,
                        error = %err,
                        "upload part failed"
                    );
                    if err.is_auth() {
                        return Err(err.into());
                    }
                    last_error = Some(err);
                    tokio::time::sleep(Duration::from_millis(300 * attempt as u64)).await;
                }
            }
        }
        if let Some(err) = last_error {
            return Err(err.into());
        }
    }

    check_cancel(cancel)?;
    let parts = core.inner.store.list_parts(&row.id).await?;
    let mut etags = Vec::with_capacity(parts.len());
    for part in &parts {
        let etag = part
            .etag
            .clone()
            .ok_or_else(|| CoreError::Other(format!("分片 {} 缺少 ETag", part.part_number)))?;
        etags.push(etag);
    }

    core.update_progress(&row.id, "running", "committing", done, total)
        .await?;
    client.complete_multipart(&state.pre, &etags).await?;
    client
        .upload_finish(&state.pre.data.obj_key, &state.pre.data.task_id)
        .await?;
    core.persist_session(&row.account_id, &client).await?;
    core.update_progress(&row.id, "done", "done", total, total)
        .await?;
    info!(transfer = %row.id, file = %row.remote_name, "upload complete");
    Ok(())
}

async fn run_download(
    core: &AppCore,
    row: &quark_store::TransferRow,
    cancel: &CancellationToken,
) -> Result<()> {
    check_cancel(cancel)?;
    let fid = row
        .fid
        .clone()
        .ok_or_else(|| CoreError::InvalidInput("下载任务缺少 fid".to_string()))?;
    let client = core.client_for(&row.account_id).await?;
    let mut total = row.total_bytes;
    let mut urls = client.download_urls(&[fid]).await?;
    core.persist_session(&row.account_id, &client).await?;
    let first = urls
        .pop()
        .ok_or_else(|| CoreError::Other("服务端没有返回下载地址".to_string()))?;
    let download_url = first.download_url;
    if total <= 0 {
        total = probe_total(&client, &download_url).await.unwrap_or(0);
    }
    if total <= 0 {
        return Err(CoreError::Other("无法获取文件大小".to_string()));
    }

    let final_path = PathBuf::from(&row.local_path);
    if let Some(parent) = final_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part_path = PathBuf::from(format!("{}.part", final_path.to_string_lossy()));
    let part_size = DEFAULT_PART_SIZE;
    let part_count = (total + part_size - 1) / part_size;
    ensure_download_parts(core, &row.id, total, part_size, part_count).await?;

    let existing = tokio::fs::metadata(&part_path)
        .await
        .map(|meta| meta.len() as i64)
        .unwrap_or(0);
    if existing != total {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&part_path)
            .await?;
        file.set_len(total as u64).await?;
    }

    let done = Arc::new(AtomicI64::new(parts_done_bytes(core, &row.id).await?));
    core.update_progress(
        &row.id,
        "running",
        "downloading",
        done.load(Ordering::Relaxed),
        total,
    )
    .await?;

    let parts = core.inner.store.list_parts(&row.id).await?;
    let pending = parts
        .into_iter()
        .filter(|part| part.status != "done")
        .collect::<Vec<_>>();
    if !pending.is_empty() {
        let url = Arc::new(Mutex::new(download_url.clone()));
        let results = stream::iter(pending)
            .map(|part| {
                let core = core.clone();
                let client = client.clone();
                let job = DownloadPartJob {
                    part_path: part_path.clone(),
                    url: url.clone(),
                    part,
                    done: done.clone(),
                    total,
                };
                let cancel = cancel.clone();
                async move { download_part(&core, &client, job, &cancel).await }
            })
            .buffer_unordered(3)
            .collect::<Vec<Result<()>>>()
            .await;
        for result in results {
            result?;
        }
    }

    check_cancel(cancel)?;
    let actual = tokio::fs::metadata(&part_path).await?.len() as i64;
    if actual != total {
        return Err(CoreError::Other(format!(
            "下载文件大小不一致，期望 {total}，实际 {actual}"
        )));
    }
    tokio::fs::rename(&part_path, &final_path).await?;
    core.update_progress(&row.id, "done", "done", total, total)
        .await?;
    info!(transfer = %row.id, file = %row.remote_name, "download complete");
    Ok(())
}

struct DownloadPartJob {
    part_path: std::path::PathBuf,
    url: Arc<Mutex<String>>,
    part: TransferPartRow,
    done: Arc<AtomicI64>,
    total: i64,
}

async fn download_part(
    core: &AppCore,
    client: &QuarkClient,
    job: DownloadPartJob,
    cancel: &CancellationToken,
) -> Result<()> {
    let DownloadPartJob {
        part_path,
        url,
        part,
        done,
        total,
    } = job;
    let start = part.offset.max(0) as u64;
    let size = part.size.max(0) as u64;
    let end = start + size - 1;
    let mut attempt = 0;
    let mut last_error = None;
    while attempt < 3 {
        attempt += 1;
        check_cancel(cancel)?;
        let current_url = url.lock().await.clone();
        let response = match client.download_range(&current_url, start, Some(end)).await {
            Ok(response) => response,
            Err(err) => {
                last_error = Some(CoreError::from(err));
                tokio::time::sleep(Duration::from_millis(300 * attempt as u64)).await;
                continue;
            }
        };
        let status = response.status();
        if status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::GONE
        {
            let transfer = core.inner.store.get_transfer(&part.transfer_id).await?;
            let fid = transfer
                .as_ref()
                .and_then(|row| row.fid.clone())
                .unwrap_or_default();
            let account_id = transfer
                .as_ref()
                .map(|row| row.account_id.clone())
                .unwrap_or_default();
            if !fid.is_empty() {
                if let Some(fresh) = client.download_urls(&[fid]).await?.into_iter().next() {
                    *url.lock().await = fresh.download_url;
                    if !account_id.is_empty() {
                        core.persist_session(&account_id, client).await.ok();
                    }
                    continue;
                }
            }
            last_error = Some(CoreError::Other(format!("下载直链已失效: HTTP {status}")));
            continue;
        }
        if status != reqwest::StatusCode::PARTIAL_CONTENT && start > 0 {
            last_error = Some(CoreError::Other(format!(
                "服务端未按 Range 返回分片: HTTP {status}"
            )));
            continue;
        }
        if !status.is_success() {
            last_error = Some(CoreError::Other(format!("下载分片失败: HTTP {status}")));
            tokio::time::sleep(Duration::from_millis(300 * attempt as u64)).await;
            continue;
        }

        let mut file = OpenOptions::new().write(true).open(&part_path).await?;
        file.seek(std::io::SeekFrom::Start(start)).await?;
        let mut written = 0u64;
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            check_cancel(cancel)?;
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            written += chunk.len() as u64;
        }
        if written != size {
            last_error = Some(CoreError::Other(format!(
                "分片 {} 大小不一致，期望 {size}，实际 {written}",
                part.part_number
            )));
            continue;
        }
        file.flush().await?;
        drop(file);
        core.inner
            .store
            .upsert_part(&TransferPartRow {
                status: "done".to_string(),
                ..part.clone()
            })
            .await?;
        let done_now = done.fetch_add(part.size, Ordering::Relaxed) + part.size;
        core.update_progress(&part.transfer_id, "running", "downloading", done_now, total)
            .await?;
        return Ok(());
    }
    Err(last_error.unwrap_or_else(|| CoreError::Other("下载分片失败".to_string())))
}

async fn ensure_upload_parts(
    core: &AppCore,
    transfer_id: &str,
    total: i64,
    part_size: i64,
    part_count: i64,
) -> Result<()> {
    let existing = core.inner.store.list_parts(transfer_id).await?;
    if existing.len() as i64 == part_count {
        return Ok(());
    }
    for index in 0..part_count {
        let offset = index * part_size;
        let size = (total - offset).min(part_size);
        core.inner
            .store
            .upsert_part(&TransferPartRow {
                transfer_id: transfer_id.to_string(),
                part_number: index + 1,
                offset,
                size,
                etag: None,
                status: "pending".to_string(),
            })
            .await?;
    }
    Ok(())
}

async fn ensure_download_parts(
    core: &AppCore,
    transfer_id: &str,
    total: i64,
    part_size: i64,
    part_count: i64,
) -> Result<()> {
    let existing = core.inner.store.list_parts(transfer_id).await?;
    if existing.len() as i64 == part_count {
        return Ok(());
    }
    for index in 0..part_count {
        let offset = index * part_size;
        let size = (total - offset).min(part_size);
        core.inner
            .store
            .upsert_part(&TransferPartRow {
                transfer_id: transfer_id.to_string(),
                part_number: index + 1,
                offset,
                size,
                etag: None,
                status: "pending".to_string(),
            })
            .await?;
    }
    Ok(())
}

async fn parts_done_bytes(core: &AppCore, transfer_id: &str) -> Result<i64> {
    Ok(core
        .inner
        .store
        .list_parts(transfer_id)
        .await?
        .iter()
        .filter(|part| part.status == "done")
        .map(|part| part.size)
        .sum())
}

async fn save_upload_state(
    core: &AppCore,
    transfer_id: &str,
    state: &UploadSessionState,
) -> Result<()> {
    let json = serde_json::to_string(state)?;
    core.inner
        .store
        .patch_transfer_state(
            transfer_id,
            "running",
            "uploading",
            parts_done_bytes(core, transfer_id).await?,
            None,
            Some(&json),
        )
        .await?;
    Ok(())
}

pub(crate) async fn hash_file(path: &Path, cancel: &CancellationToken) -> Result<(String, String)> {
    let mut file = File::open(path).await?;
    let mut md5 = md5::Md5::new();
    let mut sha1 = sha1::Sha1::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        check_cancel(cancel)?;
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        md5.update(&buffer[..read]);
        sha1.update(&buffer[..read]);
    }
    Ok((hex(&md5.finalize()), hex(&sha1.finalize())))
}

pub(crate) async fn read_part(path: &Path, offset: u64, size: u64) -> Result<Bytes> {
    let mut file = File::open(path).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut buffer = vec![0u8; size as usize];
    file.read_exact(&mut buffer).await?;
    Ok(Bytes::from(buffer))
}

async fn probe_total(client: &QuarkClient, url: &str) -> Result<i64> {
    let response = client.download_range(url, 0, Some(0)).await?;
    if let Some(range) = response.headers().get(reqwest::header::CONTENT_RANGE) {
        if let Ok(text) = range.to_str() {
            if let Some(total) = text.rsplit('/').next().and_then(|value| value.parse().ok()) {
                return Ok(total);
            }
        }
    }
    if let Some(length) = response.headers().get(reqwest::header::CONTENT_LENGTH) {
        if let Ok(text) = length.to_str() {
            if let Ok(value) = text.parse() {
                return Ok(value);
            }
        }
    }
    Ok(0)
}

fn check_cancel(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(CoreError::Other("任务已暂停或取消".to_string()))
    } else {
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
