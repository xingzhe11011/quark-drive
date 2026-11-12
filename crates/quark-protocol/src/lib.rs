mod auth;
mod client;
mod error;
mod models;
mod session;

pub use auth::{QrLogin, QrLoginConfig, QrLoginPoll, QR_LOGIN_TTL_MS};
pub use client::{
    build_complete_xml, header_value, part_auth_meta, set_cookie_headers, ClientConfig,
    QuarkClient, SecretClient, DEFAULT_QUARK_USER_AGENT,
};
pub use error::{Error, Result};
pub use models::{
    DownloadUrl, FileEntry, HashResult, ListPage, MemberInfo, RecycleItem, RecyclePage,
    UploadCallback, UploadPre, UploadPreData, UploadPreMetadata,
};
pub use session::Session;

pub const ROOT_FID: &str = "0";
pub const DEFAULT_SORT: &str = "file_type:asc,updated_at:desc";
