use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileEntry {
    #[serde(default)]
    pub fid: String,
    #[serde(default)]
    pub pdir_fid: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub category: i64,
    #[serde(default)]
    pub format_type: String,
    #[serde(default)]
    pub status: i64,
    #[serde(default)]
    pub file: bool,
    #[serde(default)]
    pub dir: bool,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub thumbnail: String,
}

impl FileEntry {
    pub fn is_dir(&self) -> bool {
        self.dir || !self.file
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListPage {
    pub entries: Vec<FileEntry>,
    pub total: u64,
    pub page: u32,
    pub size: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecycleItem {
    #[serde(default)]
    pub record_id: String,
    #[serde(default)]
    pub fid: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub file_type: i64,
    #[serde(default)]
    pub include_items: i64,
    #[serde(default)]
    pub category: i64,
    #[serde(default)]
    pub format_type: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub pdir_fid: String,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub move_recycle_at: i64,
}

impl RecycleItem {
    pub fn is_dir(&self) -> bool {
        self.file_type == 0
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecyclePage {
    pub entries: Vec<RecycleItem>,
    pub total: u64,
    pub page: u32,
    pub size: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemberInfo {
    #[serde(default)]
    pub uid: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub avatar_url: String,
    #[serde(default)]
    pub member_type: String,
    #[serde(default)]
    pub mobile: String,
    #[serde(default)]
    pub total_capacity: i64,
    #[serde(default, alias = "used_capacity")]
    pub use_capacity: i64,
    #[serde(default)]
    pub expire_at: Option<i64>,
}

impl MemberInfo {
    pub fn is_vip(&self) -> bool {
        let kind = self.member_type.to_ascii_uppercase();
        kind.contains("VIP") || kind.contains("SUPER") || kind.contains("MEMBER")
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DownloadUrl {
    #[serde(default)]
    pub fid: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub download_url: String,
    #[serde(default)]
    pub md5: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadCallback {
    #[serde(default, rename = "callbackUrl")]
    pub callback_url: String,
    #[serde(default, rename = "callbackBody")]
    pub callback_body: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadPreData {
    #[serde(default)]
    pub finish: bool,
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub upload_id: String,
    #[serde(default)]
    pub auth_info: String,
    #[serde(default)]
    pub upload_url: String,
    #[serde(default)]
    pub obj_key: String,
    #[serde(default)]
    pub fid: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub format_type: String,
    #[serde(default)]
    pub callback: UploadCallback,
    #[serde(default)]
    pub auth_info_expried: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadPreMetadata {
    #[serde(default)]
    pub part_size: i64,
    #[serde(default)]
    pub part_thread: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadPre {
    #[serde(default)]
    pub data: UploadPreData,
    #[serde(default)]
    pub metadata: UploadPreMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HashResult {
    #[serde(default)]
    pub finish: bool,
}
