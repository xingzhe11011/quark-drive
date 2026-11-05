use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{FromRow, Row, SqlitePool};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("invalid database path: {0}")]
    Path(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, Default, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AccountRow {
    pub id: String,
    pub uid: String,
    pub nickname: String,
    pub avatar_url: String,
    pub vip_type: String,
    pub vip_expire_at: Option<i64>,
    pub total_space: i64,
    pub used_space: i64,
    pub cookie_ref: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_check_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TransferRow {
    pub id: String,
    pub account_id: String,
    pub kind: String,
    pub status: String,
    pub phase: String,
    pub fid: Option<String>,
    pub local_path: String,
    pub remote_name: String,
    pub remote_parent_fid: String,
    pub total_bytes: i64,
    pub done_bytes: i64,
    pub error_code: Option<String>,
    pub error_msg: Option<String>,
    pub session_json: Option<String>,
    pub priority: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TransferPartRow {
    pub transfer_id: String,
    pub part_number: i64,
    pub offset: i64,
    pub size: i64,
    pub etag: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct FileCacheRow {
    pub account_id: String,
    pub fid: String,
    pub pdir_fid: String,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub category: i64,
    pub updated_at: i64,
    pub cached_at: i64,
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| StoreError::Path(format!("{}: {err}", parent.display())))?;
        }
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
            .map_err(|err| StoreError::Path(err.to_string()))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(std::time::Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn connect_memory() -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn upsert_account(&self, account: &AccountRow) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO accounts
                (id, uid, nickname, avatar_url, vip_type, vip_expire_at, total_space,
                 used_space, cookie_ref, created_at, updated_at, last_check_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                uid = excluded.uid,
                nickname = excluded.nickname,
                avatar_url = excluded.avatar_url,
                vip_type = excluded.vip_type,
                vip_expire_at = excluded.vip_expire_at,
                total_space = excluded.total_space,
                used_space = excluded.used_space,
                cookie_ref = excluded.cookie_ref,
                updated_at = excluded.updated_at,
                last_check_at = excluded.last_check_at
            "#,
        )
        .bind(&account.id)
        .bind(&account.uid)
        .bind(&account.nickname)
        .bind(&account.avatar_url)
        .bind(&account.vip_type)
        .bind(account.vip_expire_at)
        .bind(account.total_space)
        .bind(account.used_space)
        .bind(&account.cookie_ref)
        .bind(account.created_at)
        .bind(account.updated_at)
        .bind(account.last_check_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_accounts(&self) -> Result<Vec<AccountRow>> {
        Ok(sqlx::query_as::<_, AccountRow>(
            "SELECT * FROM accounts ORDER BY updated_at DESC, id ASC",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn get_account(&self, id: &str) -> Result<Option<AccountRow>> {
        Ok(
            sqlx::query_as::<_, AccountRow>("SELECT * FROM accounts WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn delete_account(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM accounts WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn replace_file_page(
        &self,
        account_id: &str,
        parent_fid: &str,
        files: &[FileCacheRow],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM file_cache WHERE account_id = ? AND pdir_fid = ?")
            .bind(account_id)
            .bind(parent_fid)
            .execute(&mut *tx)
            .await?;
        for file in files {
            sqlx::query(
                r#"
                INSERT INTO file_cache
                    (account_id, fid, pdir_fid, name, is_dir, size, category, updated_at, cached_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(account_id, fid) DO UPDATE SET
                    name = excluded.name,
                    is_dir = excluded.is_dir,
                    size = excluded.size,
                    category = excluded.category,
                    updated_at = excluded.updated_at,
                    cached_at = excluded.cached_at
                "#,
            )
            .bind(&file.account_id)
            .bind(&file.fid)
            .bind(&file.pdir_fid)
            .bind(&file.name)
            .bind(file.is_dir)
            .bind(file.size)
            .bind(file.category)
            .bind(file.updated_at)
            .bind(file.cached_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_cached_files(
        &self,
        account_id: &str,
        parent_fid: &str,
    ) -> Result<Vec<FileCacheRow>> {
        Ok(sqlx::query_as::<_, FileCacheRow>(
            "SELECT * FROM file_cache WHERE account_id = ? AND pdir_fid = ? ORDER BY is_dir DESC, name ASC",
        )
        .bind(account_id)
        .bind(parent_fid)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn insert_transfer(&self, transfer: &TransferRow) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO transfers
                (id, account_id, kind, status, phase, fid, local_path, remote_name,
                 remote_parent_fid, total_bytes, done_bytes, error_code, error_msg,
                 session_json, priority, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&transfer.id)
        .bind(&transfer.account_id)
        .bind(&transfer.kind)
        .bind(&transfer.status)
        .bind(&transfer.phase)
        .bind(&transfer.fid)
        .bind(&transfer.local_path)
        .bind(&transfer.remote_name)
        .bind(&transfer.remote_parent_fid)
        .bind(transfer.total_bytes)
        .bind(transfer.done_bytes)
        .bind(&transfer.error_code)
        .bind(&transfer.error_msg)
        .bind(&transfer.session_json)
        .bind(transfer.priority)
        .bind(transfer.created_at)
        .bind(transfer.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_transfer(&self, transfer: &TransferRow) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE transfers SET
                status = ?, phase = ?, fid = ?, total_bytes = ?, done_bytes = ?,
                error_code = ?, error_msg = ?, session_json = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&transfer.status)
        .bind(&transfer.phase)
        .bind(&transfer.fid)
        .bind(transfer.total_bytes)
        .bind(transfer.done_bytes)
        .bind(&transfer.error_code)
        .bind(&transfer.error_msg)
        .bind(&transfer.session_json)
        .bind(transfer.updated_at)
        .bind(&transfer.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn patch_transfer_state(
        &self,
        id: &str,
        status: &str,
        phase: &str,
        done_bytes: i64,
        error: Option<(&str, &str)>,
        session_json: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE transfers SET
                status = ?, phase = ?, done_bytes = ?, error_code = ?, error_msg = ?,
                session_json = COALESCE(?, session_json), updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(status)
        .bind(phase)
        .bind(done_bytes)
        .bind(error.map(|value| value.0))
        .bind(error.map(|value| value.1))
        .bind(session_json)
        .bind(now_millis())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_transfer(&self, id: &str) -> Result<Option<TransferRow>> {
        Ok(
            sqlx::query_as::<_, TransferRow>("SELECT * FROM transfers WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn list_transfers(&self, limit: i64) -> Result<Vec<TransferRow>> {
        Ok(sqlx::query_as::<_, TransferRow>(
            "SELECT * FROM transfers ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn recoverable_transfers(&self) -> Result<Vec<TransferRow>> {
        Ok(sqlx::query_as::<_, TransferRow>(
            "SELECT * FROM transfers WHERE status IN ('queued', 'running', 'preparing', 'committing', 'paused') ORDER BY priority DESC, created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn delete_transfer(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM transfers WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear_finished_transfers(&self) -> Result<()> {
        sqlx::query("DELETE FROM transfers WHERE status IN ('done', 'canceled', 'failed')")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn upsert_part(&self, part: &TransferPartRow) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO transfer_parts
                (transfer_id, part_number, offset, size, etag, status)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(transfer_id, part_number) DO UPDATE SET
                offset = excluded.offset,
                size = excluded.size,
                etag = excluded.etag,
                status = excluded.status
            "#,
        )
        .bind(&part.transfer_id)
        .bind(part.part_number)
        .bind(part.offset)
        .bind(part.size)
        .bind(&part.etag)
        .bind(&part.status)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_parts(&self, transfer_id: &str) -> Result<Vec<TransferPartRow>> {
        Ok(sqlx::query_as::<_, TransferPartRow>(
            "SELECT * FROM transfer_parts WHERE transfer_id = ? ORDER BY part_number ASC",
        )
        .bind(transfer_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn reset_running_parts(&self, transfer_id: &str) -> Result<()> {
        sqlx::query("UPDATE transfer_parts SET status = 'pending' WHERE transfer_id = ? AND status = 'running'")
            .bind(transfer_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get::<String, _>("value")))
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

pub fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("quark-drive")
}
