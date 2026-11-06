use quark_core::{
    AccountView, Bootstrap, DownloadItem, FileView, ListResult, QrLoginPollView, QrLoginStart,
    RecycleListResult, Settings, TransferView,
};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

use crate::AppState;

type CommandResult<T> = std::result::Result<T, String>;

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
pub async fn get_bootstrap(state: State<'_, AppState>) -> CommandResult<Bootstrap> {
    state.core.bootstrap().await.map_err(err)
}

#[tauri::command]
pub async fn start_qr_login(state: State<'_, AppState>) -> CommandResult<QrLoginStart> {
    state.core.start_qr_login().await.map_err(err)
}

#[tauri::command]
pub async fn poll_qr_login(
    state: State<'_, AppState>,
    session_id: String,
) -> CommandResult<QrLoginPollView> {
    state.core.poll_qr_login(&session_id).await.map_err(err)
}

#[tauri::command]
pub async fn cancel_qr_login(state: State<'_, AppState>, session_id: String) -> CommandResult<()> {
    state.core.cancel_qr_login(&session_id).await.map_err(err)
}

#[tauri::command]
pub async fn import_cookie(
    state: State<'_, AppState>,
    cookie: String,
) -> CommandResult<AccountView> {
    state
        .core
        .login_with_cookie(cookie.trim())
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>, account_id: String) -> CommandResult<()> {
    state.core.logout(&account_id).await.map_err(err)
}

#[tauri::command]
pub async fn refresh_account(
    state: State<'_, AppState>,
    account_id: String,
) -> CommandResult<AccountView> {
    state.core.refresh_account(&account_id).await.map_err(err)
}

#[tauri::command]
pub async fn list_dir(
    state: State<'_, AppState>,
    account_id: String,
    parent_fid: String,
    page: u32,
    size: u32,
) -> CommandResult<ListResult> {
    state
        .core
        .list_dir(&account_id, &parent_fid, page, size)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn cached_files(
    state: State<'_, AppState>,
    account_id: String,
    parent_fid: String,
) -> CommandResult<Vec<FileView>> {
    state
        .core
        .cached_files(&account_id, &parent_fid)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn mkdir(
    state: State<'_, AppState>,
    account_id: String,
    parent_fid: String,
    name: String,
) -> CommandResult<String> {
    state
        .core
        .mkdir(&account_id, &parent_fid, &name)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn rename(
    state: State<'_, AppState>,
    account_id: String,
    fid: String,
    name: String,
) -> CommandResult<()> {
    state
        .core
        .rename(&account_id, &fid, &name)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn move_files(
    state: State<'_, AppState>,
    account_id: String,
    fids: Vec<String>,
    parent_fid: String,
) -> CommandResult<()> {
    state
        .core
        .move_files(&account_id, &fids, &parent_fid)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn delete_files(
    state: State<'_, AppState>,
    account_id: String,
    fids: Vec<String>,
    permanent: bool,
) -> CommandResult<()> {
    state
        .core
        .delete_files(&account_id, &fids, permanent)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn list_recycle(
    state: State<'_, AppState>,
    account_id: String,
    page: u32,
    size: u32,
) -> CommandResult<RecycleListResult> {
    state
        .core
        .list_recycle(&account_id, page, size)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn restore_recycle(
    state: State<'_, AppState>,
    account_id: String,
    record_ids: Vec<String>,
) -> CommandResult<()> {
    state
        .core
        .restore_recycle(&account_id, &record_ids)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn delete_recycle(
    state: State<'_, AppState>,
    account_id: String,
    record_ids: Vec<String>,
) -> CommandResult<()> {
    state
        .core
        .delete_recycle(&account_id, &record_ids)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn clear_recycle(state: State<'_, AppState>, account_id: String) -> CommandResult<u64> {
    state.core.clear_recycle(&account_id).await.map_err(err)
}

#[tauri::command]
pub async fn enqueue_upload(
    state: State<'_, AppState>,
    account_id: String,
    paths: Vec<String>,
    parent_fid: String,
) -> CommandResult<Vec<String>> {
    state
        .core
        .enqueue_upload(&account_id, paths, &parent_fid)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn enqueue_download(
    state: State<'_, AppState>,
    account_id: String,
    items: Vec<DownloadItem>,
    dest_dir: Option<String>,
) -> CommandResult<Vec<String>> {
    state
        .core
        .enqueue_download(&account_id, items, dest_dir)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn list_transfers(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> CommandResult<Vec<TransferView>> {
    state
        .core
        .list_transfers(limit.unwrap_or(200))
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn pause_transfer(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.core.pause_transfer(&id).await.map_err(err)
}

#[tauri::command]
pub async fn resume_transfer(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.core.resume_transfer(&id).await.map_err(err)
}

#[tauri::command]
pub async fn cancel_transfer(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.core.cancel_transfer(&id).await.map_err(err)
}

#[tauri::command]
pub async fn retry_transfer(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.core.retry_transfer(&id).await.map_err(err)
}

#[tauri::command]
pub async fn remove_transfer(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.core.remove_transfer(&id).await.map_err(err)
}

#[tauri::command]
pub async fn clear_finished_transfers(state: State<'_, AppState>) -> CommandResult<()> {
    state.core.clear_finished_transfers().await.map_err(err)
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CommandResult<Settings> {
    Ok(state.core.settings().await)
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    download_dir: Option<String>,
    concurrency: Option<u32>,
    max_speed_bps: Option<u64>,
    theme: Option<String>,
) -> CommandResult<Settings> {
    state
        .core
        .update_settings(download_dir, concurrency, max_speed_bps, theme)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn pick_files(app: AppHandle) -> CommandResult<Vec<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = rfd::FileDialog::new()
            .pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        let _ = tx.send(result);
    })
    .map_err(err)?;
    rx.await.map_err(err)
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> CommandResult<Option<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = rfd::FileDialog::new()
            .pick_folder()
            .map(|path| path.to_string_lossy().to_string());
        let _ = tx.send(result);
    })
    .map_err(err)?;
    rx.await.map_err(err)
}

#[tauri::command]
pub async fn open_path(path: String) -> CommandResult<()> {
    let requested = PathBuf::from(path);
    let folder = folder_for_path(&requested);
    let folder = std::fs::canonicalize(&folder)
        .map_err(|error| format!("无法访问文件夹 {}：{error}", folder.display()))?;
    if !folder.is_dir() {
        return Err(format!("不是文件夹：{}", folder.display()));
    }
    open_folder(&folder)
}

fn folder_for_path(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(path)
            .to_path_buf()
    }
}

fn folder_open_command(program: &str, folder: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    if program == "gio" {
        command.arg("open");
    }
    command.arg(folder);
    command
}

fn open_folder(folder: &Path) -> CommandResult<()> {
    let mut errors = Vec::new();
    for program in ["xdg-open", "gio"] {
        match folder_open_command(program, folder).status() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => errors.push(format!("{program} 退出码 {}", status.code().unwrap_or(-1))),
            Err(error) => errors.push(format!("{program}: {error}")),
        }
    }
    Err(format!("无法打开文件夹：{}", errors.join("；")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn file_uses_parent_folder() {
        let folder = folder_for_path(Path::new("/tmp/quark-drive/file.zip"));
        assert_eq!(folder, PathBuf::from("/tmp/quark-drive"));
    }

    #[test]
    fn directory_uses_itself() {
        let folder = folder_for_path(&std::env::temp_dir());
        assert_eq!(folder, std::env::temp_dir());
    }

    #[test]
    fn xdg_open_does_not_receive_option_separator() {
        let command = folder_open_command("xdg-open", Path::new("/tmp/quark-drive"));
        assert_eq!(command.get_program(), "xdg-open");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("/tmp/quark-drive")]
        );
    }

    #[test]
    fn gio_receives_open_subcommand() {
        let command = folder_open_command("gio", Path::new("/tmp/quark-drive"));
        assert_eq!(command.get_program(), "gio");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("open"), OsStr::new("/tmp/quark-drive")]
        );
    }
}
