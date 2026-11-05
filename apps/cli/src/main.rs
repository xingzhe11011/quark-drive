use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use quark_protocol::{ClientConfig, QuarkClient, Session};
use sha2::Digest;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Parser, Debug)]
#[command(name = "quark-probe", version, about = "夸克网盘协议探针与命令行工具")]
struct Args {
    #[arg(long, env = "QUARK_COOKIE")]
    cookie: Option<String>,
    #[arg(long)]
    cookie_file: Option<PathBuf>,
    #[arg(long, default_value = "https://drive.quark.cn/1/clouddrive")]
    api_base: String,
    #[arg(long, default_value = "https://drive-pc.quark.cn/1/clouddrive")]
    pc_api_base: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 打印账号信息和容量
    Member,
    /// 列出目录
    Ls {
        #[arg(long, default_value = "0")]
        fid: String,
        #[arg(long, default_value_t = 1)]
        page: u32,
        #[arg(long, default_value_t = 100)]
        size: u32,
    },
    /// 下载文件
    Download {
        #[arg(long)]
        fid: String,
        #[arg(long, short)]
        output: PathBuf,
        #[arg(long, default_value_t = false)]
        resume: bool,
    },
    /// 上传文件
    Upload {
        #[arg(long)]
        file: PathBuf,
        #[arg(long, default_value = "0")]
        parent: String,
    },
    /// 新建文件夹
    Mkdir {
        #[arg(long)]
        parent: String,
        #[arg(long)]
        name: String,
    },
    /// 重命名
    Rename {
        #[arg(long)]
        fid: String,
        #[arg(long)]
        name: String,
    },
    /// 删除到回收站
    Delete {
        #[arg(long)]
        fid: Vec<String>,
        #[arg(long, default_value_t = false)]
        permanent: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let args = Args::parse();
    let cookie = read_cookie(&args)?;
    let session = Session::from_cookie_header(&cookie)?;
    let client = QuarkClient::new(
        session,
        ClientConfig {
            api_base: args.api_base,
            pc_api_base: args.pc_api_base,
            ..ClientConfig::default()
        },
    )?;

    match args.command {
        Command::Member => {
            let member = client.member_info().await?;
            println!("{}", serde_json::to_string_pretty(&member)?);
        }
        Command::Ls { fid, page, size } => {
            let result = client
                .list_dir(&fid, page, size, quark_protocol::DEFAULT_SORT)
                .await?;
            println!("{:<24} {:<6} {:>14}  名称", "FID", "类型", "大小");
            for entry in &result.entries {
                println!(
                    "{:<24} {:<6} {:>14}  {}",
                    entry.fid,
                    if entry.is_dir() { "目录" } else { "文件" },
                    if entry.is_dir() {
                        "-".to_string()
                    } else {
                        humans(entry.size.max(0) as u64)
                    },
                    entry.file_name
                );
            }
            println!("共 {} 项，当前第 {} 页", result.total, result.page);
        }
        Command::Download {
            fid,
            output,
            resume,
        } => {
            let url = client
                .download_urls(&[fid])
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("没有下载地址"))?;
            download(&client, &url.download_url, &output, resume).await?;
            println!("已下载到 {}", output.display());
        }
        Command::Upload { file, parent } => {
            upload(&client, &file, &parent).await?;
            println!("上传完成: {}", file.display());
            let name = file
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let page = client
                .list_dir(&parent, 1, 200, quark_protocol::DEFAULT_SORT)
                .await?;
            if let Some(entry) = page.entries.iter().find(|entry| entry.file_name == name) {
                println!("FID: {}", entry.fid);
            }
        }
        Command::Mkdir { parent, name } => {
            let fid = client.mkdir(&parent, &name).await?;
            println!("已创建 {fid}");
        }
        Command::Rename { fid, name } => {
            client.rename(&fid, &name).await?;
            println!("已重命名");
        }
        Command::Delete { fid, permanent } => {
            client.delete_files(&fid, permanent).await?;
            println!("已删除 {} 项", fid.len());
        }
    }
    Ok(())
}

fn read_cookie(args: &Args) -> Result<String> {
    if let Some(cookie) = &args.cookie {
        return Ok(cookie.trim().to_string());
    }
    if let Some(path) = &args.cookie_file {
        return Ok(std::fs::read_to_string(path)
            .with_context(|| format!("读取 Cookie 文件失败: {}", path.display()))?
            .trim()
            .to_string());
    }
    Err(anyhow!(
        "请通过 --cookie 或 --cookie-file 提供 Cookie，也可设置 QUARK_COOKIE"
    ))
}

async fn download(client: &QuarkClient, url: &str, output: &Path, resume: bool) -> Result<()> {
    let start = if resume && output.exists() {
        output.metadata()?.len()
    } else {
        0
    };
    if start > 0 {
        println!("从 {} 字节处续传", start);
    }
    let response = client.download_range(url, start, None).await?;
    if !response.status().is_success() {
        return Err(anyhow!("下载失败: HTTP {}", response.status()));
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(start > 0)
        .write(true)
        .truncate(start == 0)
        .open(output)
        .await?;
    let mut stream = response.bytes_stream();
    let mut written = start;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        written += chunk.len() as u64;
        if written % (16 * 1024 * 1024) < chunk.len() as u64 {
            print!("\r已下载 {}", humans(written));
        }
    }
    file.flush().await?;
    println!("\r已下载 {}", humans(written));
    Ok(())
}

async fn upload(client: &QuarkClient, path: &Path, parent: &str) -> Result<()> {
    let size = path.metadata()?.len() as i64;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("文件名无效"))?
        .to_string();
    let mime = mime_guess::from_path(path)
        .first_raw()
        .unwrap_or("application/octet-stream")
        .to_string();
    let (md5, sha1) = hash_file(path).await?;
    println!("MD5 {md5}\nSHA1 {sha1}");
    let pre = client.upload_pre(&name, size, parent, &mime).await?;
    if pre.data.finish {
        println!("秒传成功");
        return Ok(());
    }
    if client.update_hash(&md5, &sha1, &pre.data.task_id).await? {
        println!("秒传成功");
        return Ok(());
    }
    let part_size = if pre.metadata.part_size > 0 {
        pre.metadata.part_size
    } else {
        8 * 1024 * 1024
    };
    let part_count = if size == 0 {
        0
    } else {
        (size + part_size - 1) / part_size
    };
    let mut etags = Vec::new();
    for index in 0..part_count {
        let offset = index as u64 * part_size as u64;
        let length = (size as u64 - offset).min(part_size as u64);
        let bytes = read_bytes(path, offset, length).await?;
        let time_str = httpdate::fmt_http_date(SystemTime::now());
        let etag = client
            .upload_part(&pre, (index + 1) as u32, &mime, &time_str, bytes)
            .await?;
        etags.push(etag);
        print!("\r已上传 {}/{} 分片", index + 1, part_count);
    }
    println!();
    client.complete_multipart(&pre, &etags).await?;
    client
        .upload_finish(&pre.data.obj_key, &pre.data.task_id)
        .await?;
    Ok(())
}

async fn hash_file(path: &Path) -> Result<(String, String)> {
    let mut file = File::open(path).await?;
    let mut md5 = md5::Md5::new();
    let mut sha1 = sha1::Sha1::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        md5.update(&buffer[..read]);
        sha1.update(&buffer[..read]);
    }
    Ok((hex(&md5.finalize()), hex(&sha1.finalize())))
}

async fn read_bytes(path: &Path, offset: u64, size: u64) -> Result<Bytes> {
    use tokio::io::AsyncSeekExt;
    let mut file = File::open(path).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut buffer = vec![0u8; size as usize];
    file.read_exact(&mut buffer).await?;
    Ok(Bytes::from(buffer))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn humans(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.2} {}", UNITS[unit])
}
