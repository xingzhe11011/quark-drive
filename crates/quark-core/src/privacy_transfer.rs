use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use quark_protocol::{FileEntry, SecretClient};
use quark_store::now_millis;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use tracing::info;
use uuid::Uuid;

use crate::privacy::until_locked;
use crate::{fs, AppCore, CoreError, CoreEvent, DownloadItem, Result, SpeedState, TransferView};

pub(crate) const PRIVACY_TRANSFER_PREFIX: &str = "privacy:";
const PRIVACY_ROOT_FID: &str = "2";
const MIN_PART_SIZE: i64 = 1024 * 1024;
const DEFAULT_PART_SIZE: i64 = 8 * 1024 * 1024;

pub(crate) fn is_privacy_transfer(id: &str) -> bool {
    id.starts_with(PRIVACY_TRANSFER_PREFIX)
}

#[derive(Debug, Clone)]
pub(crate) enum PrivacyJobSpec {
    Download {
        fid: String,
        target: PathBuf,
        size: i64,
    },
    Upload {
        source: PathBuf,
        remote_name: String,
        parent_fid: String,
        size: i64,
    },
}

struct PrivacyJob {
    account_id: String,
    view: TransferView,
    spec: PrivacyJobSpec,
    cancel: CancellationToken,
}

#[derive(Default)]
pub(crate) struct PrivacyTransfers {
    jobs: HashMap<String, PrivacyJob>,
}

impl PrivacyTransfers {
    fn insert(&mut self, account_id: &str, spec: PrivacyJobSpec) -> (String, TransferView) {
        let now = now_millis();
        let id = format!("{PRIVACY_TRANSFER_PREFIX}{}", Uuid::new_v4());
        let (kind, fid, local_path, remote_name, parent_fid, total_bytes) = match &spec {
            PrivacyJobSpec::Download { fid, target, size } => (
                "download",
                Some(fid.clone()),
                target.to_string_lossy().to_string(),
                target
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("隐私文件")
                    .to_string(),
                PRIVACY_ROOT_FID.to_string(),
                *size,
            ),
            PrivacyJobSpec::Upload {
                source,
                remote_name,
                parent_fid,
                size,
            } => (
                "upload",
                None,
                source.to_string_lossy().to_string(),
                remote_name.clone(),
                parent_fid.clone(),
                *size,
            ),
        };
        let view = TransferView {
            id: id.clone(),
            account_id: account_id.to_string(),
            kind: kind.to_string(),
            status: "queued".to_string(),
            phase: "queued".to_string(),
            fid,
            local_path,
            remote_name,
            remote_parent_fid: parent_fid,
            total_bytes,
            done_bytes: 0,
            speed_bps: 0.0,
            error_code: None,
            error_message: None,
            created_at: now,
            updated_at: now,
        };
        self.jobs.insert(
            id.clone(),
            PrivacyJob {
                account_id: account_id.to_string(),
                view: view.clone(),
                spec,
                cancel: CancellationToken::new(),
            },
        );
        (id, view)
    }

    fn views(&self) -> Vec<TransferView> {
        let mut views = self
            .jobs
            .values()
            .map(|job| job.view.clone())
            .collect::<Vec<_>>();
        views.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.created_at.cmp(&left.created_at))
        });
        views
    }

    fn begin_run(
        &mut self,
        id: &str,
    ) -> Option<(String, PrivacyJobSpec, CancellationToken, TransferView)> {
        let job = self.jobs.get_mut(id)?;
        if matches!(job.view.status.as_str(), "running" | "done" | "canceled") {
            return None;
        }
        let token = CancellationToken::new();
        job.cancel = token.clone();
        job.view.status = "queued".to_string();
        job.view.phase = "queued".to_string();
        job.view.error_code = None;
        job.view.error_message = None;
        job.view.done_bytes = 0;
        job.view.speed_bps = 0.0;
        job.view.updated_at = now_millis();
        Some((
            job.account_id.clone(),
            job.spec.clone(),
            token,
            job.view.clone(),
        ))
    }

    fn ensure_resumable(&self, id: &str) -> Result<()> {
        let job = self.jobs.get(id).ok_or_else(|| transfer_not_found(id))?;
        if !matches!(job.view.status.as_str(), "paused" | "failed") {
            return Err(CoreError::InvalidInput("该任务已经结束".to_string()));
        }
        Ok(())
    }

    fn update_progress(
        &mut self,
        id: &str,
        token: &CancellationToken,
        status: &str,
        phase: &str,
        done_bytes: i64,
        total_bytes: i64,
    ) -> Result<TransferView> {
        let job = self.jobs.get_mut(id).ok_or_else(transfer_canceled)?;
        if token.is_cancelled() || job.cancel.is_cancelled() {
            return Err(transfer_canceled());
        }
        job.view.status = status.to_string();
        job.view.phase = phase.to_string();
        job.view.done_bytes = done_bytes;
        job.view.total_bytes = total_bytes;
        job.view.updated_at = now_millis();
        Ok(job.view.clone())
    }

    fn finish(
        &mut self,
        id: &str,
        token: &CancellationToken,
        result: &Result<()>,
    ) -> Option<(TransferView, Option<String>)> {
        let job = self.jobs.get_mut(id)?;
        if token.is_cancelled() || job.cancel.is_cancelled() {
            return None;
        }
        let error = match result {
            Ok(()) => {
                job.view.status = "done".to_string();
                job.view.phase = "done".to_string();
                job.view.done_bytes = job.view.total_bytes.max(job.view.done_bytes);
                job.view.error_code = None;
                job.view.error_message = None;
                job.view.speed_bps = 0.0;
                None
            }
            Err(error) => {
                let message = privacy_transfer_error(error);
                job.view.status = "failed".to_string();
                job.view.phase = "failed".to_string();
                job.view.speed_bps = 0.0;
                job.view.error_code = Some("privacy_transfer_failed".to_string());
                job.view.error_message = Some(message.clone());
                Some(message)
            }
        };
        job.view.updated_at = now_millis();
        Some((job.view.clone(), error))
    }

    fn pause(&mut self, id: &str) -> Result<TransferView> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| transfer_not_found(id))?;
        if matches!(
            job.view.status.as_str(),
            "done" | "canceled" | "failed" | "paused"
        ) {
            return Err(CoreError::InvalidInput("该任务当前无法暂停".to_string()));
        }
        job.cancel.cancel();
        job.view.status = "paused".to_string();
        job.view.phase = "paused".to_string();
        job.view.speed_bps = 0.0;
        job.view.updated_at = now_millis();
        Ok(job.view.clone())
    }

    fn cancel(&mut self, id: &str) -> Result<TransferView> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| transfer_not_found(id))?;
        job.cancel.cancel();
        job.view.status = "canceled".to_string();
        job.view.phase = "canceled".to_string();
        job.view.speed_bps = 0.0;
        job.view.error_code = Some("canceled".to_string());
        job.view.error_message = Some("已取消".to_string());
        job.view.updated_at = now_millis();
        Ok(job.view.clone())
    }

    fn remove(&mut self, id: &str) -> Result<()> {
        let job = self.jobs.remove(id).ok_or_else(|| transfer_not_found(id))?;
        job.cancel.cancel();
        Ok(())
    }

    fn clear_finished(&mut self) {
        self.jobs
            .retain(|_, job| !matches!(job.view.status.as_str(), "done" | "canceled" | "failed"));
    }

    fn cancel_account(&mut self, account_id: &str) -> (Vec<TransferView>, Vec<CancellationToken>) {
        let ids = self
            .jobs
            .iter()
            .filter(|(_, job)| job.account_id == account_id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut views = Vec::with_capacity(ids.len());
        let mut tokens = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(job) = self.jobs.remove(&id) {
                tokens.push(job.cancel.clone());
                let mut view = job.view;
                view.status = "canceled".to_string();
                view.phase = "canceled".to_string();
                view.fid = None;
                view.local_path.clear();
                view.remote_name = "隐私空间任务".to_string();
                view.remote_parent_fid.clear();
                view.done_bytes = 0;
                view.total_bytes = 0;
                view.speed_bps = 0.0;
                view.error_code = Some("privacy_locked".to_string());
                view.error_message = Some("隐私空间已锁定，任务已取消".to_string());
                view.updated_at = now_millis();
                views.push(view);
            }
        }
        (views, tokens)
    }
}

struct PreparedUpload {
    source: PathBuf,
    remote_name: String,
    parent_fid: String,
    size: i64,
}

struct PreparedDownload {
    fid: String,
    target: PathBuf,
    size: i64,
}

impl AppCore {
    pub fn privacy_transfer_views(&self) -> Vec<TransferView> {
        self.inner
            .privacy_transfers
            .lock()
            .expect("privacy transfer mutex poisoned")
            .views()
    }

    pub(crate) fn cancel_privacy_transfers(&self, account_id: &str) {
        let (views, tokens) = self
            .inner
            .privacy_transfers
            .lock()
            .expect("privacy transfer mutex poisoned")
            .cancel_account(account_id);
        for token in tokens {
            token.cancel();
        }
        if views.is_empty() {
            return;
        }
        {
            let mut speeds = self.inner.speeds.lock().expect("speeds mutex poisoned");
            for view in &views {
                speeds.remove(&view.id);
            }
        }
        for view in views {
            self.emit(CoreEvent::TransferChanged {
                transfer: Box::new(view),
            });
        }
    }

    pub(crate) fn spawn_privacy_transfer(&self, id: String) {
        let started = self
            .inner
            .privacy_transfers
            .lock()
            .expect("privacy transfer mutex poisoned")
            .begin_run(&id);
        let Some((account_id, spec, cancel, view)) = started else {
            return;
        };
        self.emit(CoreEvent::TransferChanged {
            transfer: Box::new(view),
        });
        let core = self.clone();
        let semaphore = self
            .inner
            .semaphore
            .lock()
            .expect("semaphore mutex poisoned")
            .clone();
        tokio::spawn(async move {
            let _permit = semaphore.acquire().await;
            let result = if cancel.is_cancelled() {
                Err(transfer_canceled())
            } else {
                run_privacy_transfer(
                    core.clone(),
                    id.clone(),
                    account_id.clone(),
                    spec.clone(),
                    cancel.clone(),
                )
                .await
            };
            core.finish_privacy_transfer(&id, &account_id, &spec, result, &cancel)
                .await;
        });
    }

    pub(crate) async fn update_privacy_progress(
        &self,
        id: &str,
        cancel: &CancellationToken,
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
        let mut transfers = self
            .inner
            .privacy_transfers
            .lock()
            .expect("privacy transfer mutex poisoned");
        let mut view =
            transfers.update_progress(id, cancel, status, phase, done_bytes, total_bytes)?;
        view.speed_bps = speed;
        drop(transfers);
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

    async fn finish_privacy_transfer(
        &self,
        id: &str,
        account_id: &str,
        spec: &PrivacyJobSpec,
        result: Result<()>,
        cancel: &CancellationToken,
    ) {
        if result.is_err() || cancel.is_cancelled() {
            cleanup_privacy_partial(spec).await;
        }
        let finished = self
            .inner
            .privacy_transfers
            .lock()
            .expect("privacy transfer mutex poisoned")
            .finish(id, cancel, &result);
        self.inner
            .speeds
            .lock()
            .expect("speeds mutex poisoned")
            .remove(id);
        let Some((view, _error)) = finished else {
            return;
        };
        if result.is_ok() {
            info!(
                transfer = %id,
                account = %account_id,
                "privacy transfer complete"
            );
        }
        self.emit(CoreEvent::TransferChanged {
            transfer: Box::new(view),
        });
    }

    pub(crate) fn pause_privacy_transfer(&self, id: &str) -> Result<()> {
        let view = self
            .inner
            .privacy_transfers
            .lock()
            .expect("privacy transfer mutex poisoned")
            .pause(id)?;
        self.inner
            .speeds
            .lock()
            .expect("speeds mutex poisoned")
            .remove(id);
        self.emit(CoreEvent::TransferChanged {
            transfer: Box::new(view),
        });
        Ok(())
    }

    pub(crate) fn resume_privacy_transfer(&self, id: &str) -> Result<()> {
        self.inner
            .privacy_transfers
            .lock()
            .expect("privacy transfer mutex poisoned")
            .ensure_resumable(id)?;
        self.spawn_privacy_transfer(id.to_string());
        Ok(())
    }

    pub(crate) fn cancel_privacy_transfer(&self, id: &str) -> Result<()> {
        let view = self
            .inner
            .privacy_transfers
            .lock()
            .expect("privacy transfer mutex poisoned")
            .cancel(id)?;
        self.inner
            .speeds
            .lock()
            .expect("speeds mutex poisoned")
            .remove(id);
        self.emit(CoreEvent::TransferChanged {
            transfer: Box::new(view),
        });
        Ok(())
    }

    pub(crate) fn remove_privacy_transfer(&self, id: &str) -> Result<()> {
        self.inner
            .privacy_transfers
            .lock()
            .expect("privacy transfer mutex poisoned")
            .remove(id)?;
        self.inner
            .speeds
            .lock()
            .expect("speeds mutex poisoned")
            .remove(id);
        Ok(())
    }

    pub(crate) fn clear_finished_privacy_transfers(&self) {
        self.inner
            .privacy_transfers
            .lock()
            .expect("privacy transfer mutex poisoned")
            .clear_finished();
    }

    pub async fn privacy_enqueue_upload(
        &self,
        account_id: &str,
        paths: Vec<String>,
        parent_fid: &str,
    ) -> Result<Vec<String>> {
        if paths.is_empty() {
            return Err(CoreError::InvalidInput("没有选择文件".to_string()));
        }
        let (epoch, session) = self.privacy_unlocked(account_id).await?;
        let prepared = until_locked(
            &epoch,
            prepare_privacy_uploads(&session, &paths, parent_fid),
        )
        .await?;
        self.validate_privacy_epoch(account_id, &epoch)?;

        let mut created = Vec::with_capacity(prepared.len());
        for upload in prepared {
            let (id, view) = self
                .inner
                .privacy_transfers
                .lock()
                .expect("privacy transfer mutex poisoned")
                .insert(
                    account_id,
                    PrivacyJobSpec::Upload {
                        source: upload.source,
                        remote_name: upload.remote_name,
                        parent_fid: upload.parent_fid,
                        size: upload.size,
                    },
                );
            created.push(id.clone());
            self.emit(CoreEvent::TransferChanged {
                transfer: Box::new(view),
            });
            self.spawn_privacy_transfer(id);
        }
        if self.validate_privacy_epoch(account_id, &epoch).is_err() {
            self.cancel_privacy_transfers(account_id);
            return Err(CoreError::Other("隐私空间已锁定，请重新解锁".to_string()));
        }
        Ok(created)
    }

    pub async fn privacy_enqueue_download(
        &self,
        account_id: &str,
        items: Vec<DownloadItem>,
        dest_dir: Option<String>,
    ) -> Result<Vec<String>> {
        if items.is_empty() {
            return Err(CoreError::InvalidInput("没有选择文件".to_string()));
        }
        let base = match dest_dir.filter(|path| !path.trim().is_empty()) {
            Some(path) => path,
            None => self.inner.settings.read().await.download_dir.clone(),
        };
        let base = PathBuf::from(base);
        std::fs::create_dir_all(&base)?;
        let (epoch, session) = self.privacy_unlocked(account_id).await?;
        let prepared =
            until_locked(&epoch, prepare_privacy_downloads(&session, items, base)).await?;
        self.validate_privacy_epoch(account_id, &epoch)?;

        let mut created = Vec::with_capacity(prepared.len());
        for download in prepared {
            let (id, view) = self
                .inner
                .privacy_transfers
                .lock()
                .expect("privacy transfer mutex poisoned")
                .insert(
                    account_id,
                    PrivacyJobSpec::Download {
                        fid: download.fid,
                        target: download.target,
                        size: download.size,
                    },
                );
            created.push(id.clone());
            self.emit(CoreEvent::TransferChanged {
                transfer: Box::new(view),
            });
            self.spawn_privacy_transfer(id);
        }
        if self.validate_privacy_epoch(account_id, &epoch).is_err() {
            self.cancel_privacy_transfers(account_id);
            return Err(CoreError::Other("隐私空间已锁定，请重新解锁".to_string()));
        }
        Ok(created)
    }
}

async fn prepare_privacy_uploads(
    session: &SecretClient,
    paths: &[String],
    parent_fid: &str,
) -> Result<Vec<PreparedUpload>> {
    let mut files = Vec::new();
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
                .ok_or_else(|| CoreError::InvalidInput("文件名无效".to_string()))?;
            let name = privacy_new_name(session, parent_fid, &fs::sanitize_file_name(name)).await?;
            files.push(PreparedUpload {
                size: source.metadata()?.len() as i64,
                source,
                remote_name: name,
                parent_fid: parent_fid.to_string(),
            });
            continue;
        }
        if !source.is_dir() {
            continue;
        }

        let root_name = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("未命名文件夹");
        let remote_root = privacy_ensure_dir(session, parent_fid, root_name).await?;
        let entries = tokio::task::spawn_blocking({
            let source = source.clone();
            move || fs::collect_local_tree(&source)
        })
        .await
        .map_err(|error| CoreError::Other(error.to_string()))??;

        let mut dir_map: HashMap<PathBuf, String> = HashMap::new();
        dir_map.insert(PathBuf::new(), remote_root);
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
                .unwrap_or_else(|| dir_map[Path::new("")].clone());
            let name = entry
                .relative
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("未命名文件夹");
            let fid = privacy_ensure_dir(session, &remote_parent, name).await?;
            dir_map.insert(entry.relative.clone(), fid);
        }
        for entry in entries.iter().filter(|entry| !entry.is_dir) {
            let parent_rel = entry.relative.parent().unwrap_or(Path::new(""));
            let remote_parent = dir_map
                .get(parent_rel)
                .cloned()
                .unwrap_or_else(|| dir_map[Path::new("")].clone());
            let name = entry
                .relative
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("未命名文件");
            let name =
                privacy_new_name(session, &remote_parent, &fs::sanitize_file_name(name)).await?;
            files.push(PreparedUpload {
                source: entry.absolute.clone(),
                remote_name: name,
                parent_fid: remote_parent,
                size: entry.size as i64,
            });
        }
    }
    Ok(files)
}

async fn prepare_privacy_downloads(
    session: &SecretClient,
    items: Vec<DownloadItem>,
    base: PathBuf,
) -> Result<Vec<PreparedDownload>> {
    let mut remote_files = Vec::new();
    let mut remote_dirs = Vec::new();
    for item in items {
        let name = fs::sanitize_file_name(&item.name);
        if item.is_dir {
            let relative = PathBuf::from(&name);
            remote_dirs.push(relative.clone());
            collect_privacy_dir(
                session,
                &item.fid,
                &relative,
                &mut remote_files,
                &mut remote_dirs,
            )
            .await?;
        } else {
            remote_files.push(PrivateRemoteFile {
                fid: item.fid,
                size: item.size,
                relative: PathBuf::from(name),
            });
        }
    }

    remote_dirs.sort_by_key(|path| path.components().count());
    for relative in remote_dirs {
        std::fs::create_dir_all(base.join(relative))?;
    }
    let mut reserved = HashSet::new();
    let mut prepared = Vec::with_capacity(remote_files.len());
    for file in remote_files {
        let target = reserve_unique_path(base.join(&file.relative), &mut reserved);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        prepared.push(PreparedDownload {
            fid: file.fid,
            target,
            size: file.size,
        });
    }
    Ok(prepared)
}

struct PrivateRemoteFile {
    fid: String,
    size: i64,
    relative: PathBuf,
}

async fn collect_privacy_dir(
    session: &SecretClient,
    fid: &str,
    relative: &Path,
    out: &mut Vec<PrivateRemoteFile>,
    out_dirs: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut queue = VecDeque::from([(fid.to_string(), relative.to_path_buf())]);
    while let Some((parent_fid, parent_relative)) = queue.pop_front() {
        let mut page_no = 1;
        loop {
            let page = session
                .list_dir(&parent_fid, page_no, 200, quark_protocol::DEFAULT_SORT)
                .await
                .map_err(|_| CoreError::Other("无法读取隐私目录".to_string()))?;
            for entry in &page.entries {
                let name = fs::sanitize_file_name(&entry.file_name);
                let child = parent_relative.join(name);
                if entry.is_dir() {
                    out_dirs.push(child.clone());
                    queue.push_back((entry.fid.clone(), child));
                } else {
                    out.push(PrivateRemoteFile {
                        fid: entry.fid.clone(),
                        size: entry.size,
                        relative: child,
                    });
                }
            }
            if page.size == 0
                || page.entries.is_empty()
                || page.entries.len() < page.size as usize
                || (page_no * page.size) as u64 >= page.total
            {
                break;
            }
            page_no += 1;
        }
    }
    Ok(())
}

async fn privacy_new_name(
    session: &SecretClient,
    parent_fid: &str,
    preferred: &str,
) -> Result<String> {
    let preferred = fs::sanitize_file_name(preferred);
    let entries = privacy_entries(session, parent_fid).await?;
    if !entries.iter().any(|entry| entry.file_name == preferred) {
        return Ok(preferred);
    }
    let (stem, extension) = split_extension(&preferred);
    for index in 1..1000 {
        let candidate = if extension.is_empty() {
            format!("{stem} ({index})")
        } else {
            format!("{stem} ({index}).{extension}")
        };
        if !entries.iter().any(|entry| entry.file_name == candidate) {
            return Ok(candidate);
        }
    }
    Err(CoreError::Other("无法生成不冲突的文件名".to_string()))
}

async fn privacy_ensure_dir(
    session: &SecretClient,
    parent_fid: &str,
    name: &str,
) -> Result<String> {
    let name = fs::sanitize_file_name(name);
    if let Some(fid) = privacy_find_child_dir(session, parent_fid, &name).await? {
        return Ok(fid);
    }
    match session.mkdir(parent_fid, &name).await {
        Ok(fid) => Ok(fid),
        Err(_) => {
            if let Some(fid) = privacy_find_child_dir(session, parent_fid, &name).await? {
                Ok(fid)
            } else {
                Err(CoreError::Other("无法创建隐私目录".to_string()))
            }
        }
    }
}

async fn privacy_find_child_dir(
    session: &SecretClient,
    parent_fid: &str,
    name: &str,
) -> Result<Option<String>> {
    Ok(privacy_entries(session, parent_fid)
        .await?
        .into_iter()
        .find(|entry| entry.file_name == name && entry.is_dir())
        .map(|entry| entry.fid))
}

async fn privacy_entries(session: &SecretClient, parent_fid: &str) -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    let mut page_no = 1_u32;
    loop {
        let page = session
            .list_dir(parent_fid, page_no, 200, quark_protocol::DEFAULT_SORT)
            .await
            .map_err(|_| CoreError::Other("无法检查隐私目录".to_string()))?;
        entries.extend(page.entries.iter().cloned());
        if page.size == 0
            || page.entries.is_empty()
            || page.entries.len() < page.size as usize
            || (page_no * page.size) as u64 >= page.total
        {
            break;
        }
        page_no += 1;
        if page_no > 1000 {
            return Err(CoreError::Other("隐私目录条目过多".to_string()));
        }
    }
    Ok(entries)
}

async fn run_privacy_transfer(
    core: AppCore,
    id: String,
    account_id: String,
    spec: PrivacyJobSpec,
    cancel: CancellationToken,
) -> Result<()> {
    check_cancel(&cancel)?;
    let (_, session) = core.privacy_unlocked(&account_id).await?;
    match spec {
        PrivacyJobSpec::Download { fid, target, size } => {
            run_privacy_download(&core, &id, session, fid, target, size, &cancel).await
        }
        PrivacyJobSpec::Upload {
            source,
            remote_name,
            parent_fid,
            size,
        } => {
            let upload = PreparedUpload {
                source,
                remote_name,
                parent_fid,
                size,
            };
            run_privacy_upload(&core, &id, session, upload, &cancel).await
        }
    }
}

async fn run_privacy_download(
    core: &AppCore,
    id: &str,
    session: SecretClient,
    fid: String,
    target: PathBuf,
    known_size: i64,
    cancel: &CancellationToken,
) -> Result<()> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut download_url = first_download_url(&session, &fid).await?;
    let mut total = known_size;
    if total <= 0 {
        total = probe_download_total(&session, &download_url)
            .await
            .unwrap_or(0);
    }
    if total <= 0 {
        return Err(CoreError::Other("无法获取隐私文件大小".to_string()));
    }

    let part_path = PathBuf::from(format!("{}.part", target.to_string_lossy()));
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&part_path)
        .await?;
    core.update_privacy_progress(id, cancel, "running", "downloading", 0, total)
        .await?;

    let part_size = DEFAULT_PART_SIZE as u64;
    let total_u64 = total as u64;
    let mut offset = 0_u64;
    while offset < total_u64 {
        check_cancel(cancel)?;
        let end = (offset + part_size - 1).min(total_u64 - 1);
        let expected = (end - offset + 1) as usize;
        let mut last_error = None;
        let mut written = false;
        for attempt in 1..=4 {
            check_cancel(cancel)?;
            let response = match session
                .download_range(&download_url, offset, Some(end))
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    last_error = Some(CoreError::from(error));
                    sleep_retry(attempt, cancel).await?;
                    continue;
                }
            };
            let status = response.status();
            if matches!(
                status,
                reqwest::StatusCode::UNAUTHORIZED
                    | reqwest::StatusCode::FORBIDDEN
                    | reqwest::StatusCode::GONE
            ) {
                match first_download_url(&session, &fid).await {
                    Ok(fresh) => {
                        download_url = fresh;
                        continue;
                    }
                    Err(error) => {
                        last_error = Some(error);
                        break;
                    }
                }
            }
            if !status.is_success()
                || (offset > 0 && status != reqwest::StatusCode::PARTIAL_CONTENT)
            {
                last_error = Some(CoreError::Other(format!(
                    "下载隐私文件分片失败: HTTP {status}"
                )));
                sleep_retry(attempt, cancel).await?;
                continue;
            }
            let body = response.bytes().await?;
            if body.len() != expected {
                last_error = Some(CoreError::Other("隐私文件分片大小不一致".to_string()));
                sleep_retry(attempt, cancel).await?;
                continue;
            }
            check_cancel(cancel)?;
            file.write_all(&body).await?;
            written = true;
            break;
        }
        if !written {
            return Err(
                last_error.unwrap_or_else(|| CoreError::Other("下载隐私文件失败".to_string()))
            );
        }
        offset = end + 1;
        core.update_privacy_progress(id, cancel, "running", "downloading", offset as i64, total)
            .await?;
    }
    file.flush().await?;
    drop(file);
    check_cancel(cancel)?;
    tokio::fs::rename(&part_path, &target).await?;
    Ok(())
}

async fn run_privacy_upload(
    core: &AppCore,
    id: &str,
    session: SecretClient,
    upload: PreparedUpload,
    cancel: &CancellationToken,
) -> Result<()> {
    let PreparedUpload {
        source,
        remote_name,
        parent_fid,
        size: known_size,
    } = upload;
    check_cancel(cancel)?;
    if !source.is_file() {
        return Err(CoreError::InvalidInput("本地文件不存在".to_string()));
    }
    let total = source.metadata()?.len() as i64;
    let total = if total > 0 { total } else { known_size };
    let mime = mime_guess::from_path(&source)
        .first_raw()
        .unwrap_or("application/octet-stream")
        .to_string();
    core.update_privacy_progress(id, cancel, "running", "hashing", 0, total)
        .await?;
    let (md5, sha1) = crate::transfer::hash_file(&source, cancel).await?;
    check_cancel(cancel)?;
    core.update_privacy_progress(id, cancel, "running", "preparing", 0, total)
        .await?;
    let pre = session
        .upload_pre(&remote_name, total, &parent_fid, &mime)
        .await
        .map_err(|_| CoreError::Other("无法准备隐私文件上传".to_string()))?;
    if pre.data.finish {
        return Ok(());
    }
    let hit = session
        .update_hash(&md5, &sha1, &pre.data.task_id)
        .await
        .map_err(|_| CoreError::Other("无法校验隐私文件".to_string()))?;
    if hit {
        return Ok(());
    }

    let part_size = if pre.metadata.part_size >= MIN_PART_SIZE {
        pre.metadata.part_size
    } else {
        DEFAULT_PART_SIZE
    };
    let part_count = if total == 0 {
        0
    } else {
        (total + part_size - 1) / part_size
    };
    let mut etags = Vec::with_capacity(part_count as usize);
    let mut done = 0_i64;
    for index in 0..part_count {
        check_cancel(cancel)?;
        let offset = index * part_size;
        let size = (total - offset).min(part_size);
        let bytes = crate::transfer::read_part(&source, offset as u64, size as u64).await?;
        let mut last_error = None;
        let mut uploaded = None;
        for attempt in 1..=4 {
            check_cancel(cancel)?;
            let time_str = httpdate::fmt_http_date(SystemTime::now());
            match session
                .upload_part(&pre, (index + 1) as u32, &mime, &time_str, bytes.clone())
                .await
            {
                Ok(etag) => {
                    uploaded = Some(etag);
                    break;
                }
                Err(error) => {
                    last_error = Some(CoreError::from(error));
                    sleep_retry(attempt, cancel).await?;
                }
            }
        }
        let Some(etag) = uploaded else {
            return Err(
                last_error.unwrap_or_else(|| CoreError::Other("隐私文件上传失败".to_string()))
            );
        };
        etags.push(etag);
        done += size;
        core.update_privacy_progress(id, cancel, "running", "uploading", done, total)
            .await?;
    }

    core.update_privacy_progress(id, cancel, "running", "committing", done, total)
        .await?;
    session
        .complete_multipart(&pre, &etags)
        .await
        .map_err(|_| CoreError::Other("无法合并隐私文件分片".to_string()))?;
    check_cancel(cancel)?;
    session
        .upload_finish(&pre.data.obj_key, &pre.data.task_id)
        .await
        .map_err(|_| CoreError::Other("无法完成隐私文件上传".to_string()))?;
    Ok(())
}

async fn first_download_url(session: &SecretClient, fid: &str) -> Result<String> {
    let mut urls = session
        .download_urls(&[fid.to_string()])
        .await
        .map_err(|_| CoreError::Other("无法获取隐私文件下载地址".to_string()))?;
    urls.pop()
        .map(|item| item.download_url)
        .filter(|url| !url.is_empty())
        .ok_or_else(|| CoreError::Other("服务端没有返回隐私文件下载地址".to_string()))
}

async fn probe_download_total(session: &SecretClient, url: &str) -> Result<i64> {
    let response = session.download_range(url, 0, Some(0)).await?;
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

async fn sleep_retry(attempt: u32, cancel: &CancellationToken) -> Result<()> {
    tokio::select! {
        _ = cancel.cancelled() => Err(transfer_canceled()),
        _ = tokio::time::sleep(Duration::from_millis(300 * attempt as u64)) => Ok(()),
    }
}

fn check_cancel(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(transfer_canceled())
    } else {
        Ok(())
    }
}

async fn cleanup_privacy_partial(spec: &PrivacyJobSpec) {
    if let PrivacyJobSpec::Download { target, .. } = spec {
        let part = PathBuf::from(format!("{}.part", target.to_string_lossy()));
        let _ = tokio::fs::remove_file(part).await;
    }
}

fn reserve_unique_path(path: PathBuf, reserved: &mut HashSet<PathBuf>) -> PathBuf {
    if !path.exists() && !reserved.contains(&path) {
        reserved.insert(path.clone());
        return path;
    }
    let parent = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("文件");
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 1..10_000 {
        let name = match extension {
            Some(ext) if !ext.is_empty() => format!("{stem} ({index}).{ext}"),
            _ => format!("{stem} ({index})"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() && !reserved.contains(&candidate) {
            reserved.insert(candidate.clone());
            return candidate;
        }
    }
    let candidate = parent.join(format!("{stem}-{}", Uuid::new_v4()));
    reserved.insert(candidate.clone());
    candidate
}

fn split_extension(name: &str) -> (String, String) {
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => {
            (stem.to_string(), extension.to_string())
        }
        _ => (name.to_string(), String::new()),
    }
}

fn privacy_transfer_error(error: &CoreError) -> String {
    match error {
        CoreError::Io(_) => "本地文件读写失败，任务已停止".to_string(),
        CoreError::InvalidInput(_) => "本地文件不可用，任务已停止".to_string(),
        _ => "隐私传输失败，请重新解锁后重试".to_string(),
    }
}

fn transfer_canceled() -> CoreError {
    CoreError::Other("隐私传输已暂停或取消".to_string())
}

fn transfer_not_found(id: &str) -> CoreError {
    CoreError::TransferNotFound(id.to_string())
}
