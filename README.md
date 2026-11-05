# QuarkDrive

夸克网盘 Linux 桌面客户端。扫码登录，支持文件浏览、回收站、上传、下载、断点续传、任务队列、限速设置和深色主题。

> 第三方客户端，非夸克官方产品，与夸克官方无任何关联。接口来自夸克网页端，可能随官方更新失效。请只操作自己的账号数据，并保留本地备份。
>
> 本项目仅用于个人账号的文件管理，不绕过会员、限速或付费能力。使用前请自行确认符合夸克服务条款；由此产生的账号或数据风险由使用者承担。

## 功能

- 应用内原生二维码登录，首次登录或会话失效时扫码，不打开夸克网页，也不依赖 WebView 读取 Cookie。
- 本地会话自动恢复；退出登录或服务端判定失效后才需要重新扫码。
- 目录浏览、面包屑、搜索、列表/网格视图、多选、新建文件夹、重命名、删除。
- 回收站：列表、恢复、彻底删除、清空。
- 上传文件/文件夹，MD5+SHA1 秒传检测，分片上传，失败重试。
- 下载文件/文件夹，三连接分片下载，直链过期自动刷新，断点续传。
- SQLite 持久化任务，重启后自动恢复。
- 传输中心：进度、速度、暂停/继续/取消/重试、清理记录。
- 设置：下载目录、并发数、限速、主题。
- 备用 Cookie 导入，便于无桌面环境或自动化验收。

## 界面

| 文件管理 | 回收站 |
| --- | --- |
| ![文件管理](docs/screenshots/files.png) | ![回收站](docs/screenshots/recycle.png) |

![设置](docs/screenshots/settings.png)

截图来自浏览器演示模式，使用虚构的演示账号与文件数据。

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面壳 | Tauri 2 |
| 前端 | React 19 + TypeScript + Vite + Tailwind CSS 4 + shadcn/ui 风格组件 |
| 后端 | Rust + Tokio + Reqwest |
| 存储 | SQLite (sqlx) + 本地 0600 凭据文件 |

```text
quark-drive/
├── crates/
│   ├── quark-protocol/   # 夸克 HTTP 协议、Cookie 轮换、上传/下载
│   ├── quark-core/       # 账号、文件、任务引擎、分片调度
│   ├── quark-store/      # SQLite 持久化
│   └── quark-testkit/    # 本地假夸克服务，用于无账号验收
├── apps/
│   ├── desktop/          # React + Tauri 桌面应用
│   └── cli/              # quark-probe 命令行探针
├── docs/screenshots/     # 界面截图
└── .github/workflows/    # CI（Rust + 前端）
```

## 开发环境

Ubuntu 22.04 / 24.04 / 26.04：

```bash
sudo apt update
sudo apt install -y \
  build-essential curl wget file pkg-config \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libssl-dev libdbus-1-dev

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Node.js 20+ 和 npm：

```bash
cd apps/desktop
npm install
```

## 开发运行

带 Vite 热更新：

```bash
cd apps/desktop
npm run tauri dev
```

生成可直接双击运行的 debug 二进制（内嵌前端，不需要 Vite）：

```bash
cd apps/desktop
npm run build:debug
```

产物为 `target/debug/quark-drive`。注意：直接执行
`cargo build -p quark-drive` 得到的是开发模式二进制，需要 Vite 在
`localhost:1420` 运行；若要单独直接运行，必须使用
`npm run build:debug` 或带 `--features custom-protocol` 构建。

仅预览前端（浏览器演示模式，不访问真实网盘）：

```bash
cd apps/desktop
npm run dev
# 打开 http://127.0.0.1:1420/，可点击“浏览器预览模式：跳过扫码进入演示界面”
```

## 构建

```bash
cd apps/desktop
npm run build
npm run tauri build -- --bundles deb,appimage
```

产物位于仓库根目录的 `target/release/bundle/`。

如果使用容器或自定义 sysroot，可按需设置：

```bash
export PKG_CONFIG_PATH="/path/to/sysroot/usr/lib/x86_64-linux-gnu/pkgconfig:/path/to/sysroot/usr/share/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR="/path/to/sysroot"
npm run build:debug
```

## 登录

1. 首次启动或登录失效时，登录页自动向夸克申请一次性二维码 token。
2. 二维码直接显示在应用登录卡片中，不会打开任何夸克网页窗口。
3. 使用夸克 App 扫码，并在手机上确认登录。
4. Rust 后台每 2 秒轮询一次状态；拿到 service ticket 后通过
   `/account/info?st=...&lw=scan` 换取 HttpOnly Cookie。
5. 登录成功后写入本地 0600 权限凭据文件，后续请求自动持久化轮换后的 `__puus`。

每次进入登录页或点击“刷新二维码”都会创建新的独立 Cookie Jar 和一次性 token。
登录成功后 Cookie 会持久化到本地，后续启动自动恢复并校验；校验失败才回到扫码页。

备用方式：登录页展开“Cookie 导入”，粘贴完整 Cookie（至少包含 `__pus` 或 `__puus`）。适合自动化验收或无 GUI 环境。

## 命令行探针

`quark-probe` 用来快速验证协议和做最终验收，不会启动 UI：

```bash
export QUARK_COOKIE='__puus=...; __pus=...; __uid=...'

cargo run -p quark-cli -- member
cargo run -p quark-cli -- ls --fid 0
cargo run -p quark-cli -- upload --file ./test.bin --parent 0
cargo run -p quark-cli -- download --fid <fid> --output ./test.download
```

也可以使用文件，避免 Cookie 进入 shell 历史：

```bash
chmod 600 cookie.txt
cargo run -p quark-cli -- --cookie-file cookie.txt member
```

## 验收

### 不依赖真实账号

```bash
# 1. 协议、上传、下载、断点续传核心测试
cargo test -p quark-protocol -p quark-core

# 2. 前端类型检查、构建、单测
cd apps/desktop
npm run build
npm test

# 3. Tauri 桌面壳编译
cd ../..
cargo check -p quark-drive
cargo build -p quark-drive
```

`quark-testkit` 提供假的夸克 API 和 OSS：

```bash
FAKE_PORT=18080 cargo run -p quark-testkit --example fake_server

# 另一个终端
cargo run -p quark-cli -- \
  --cookie '__puus=test' \
  --api-base http://127.0.0.1:18080/1/clouddrive \
  ls
```

### 依赖真实 Cookie 的验收

拿到真实 Cookie 后执行：

```bash
cargo run -p quark-cli -- --cookie-file cookie.txt member
cargo run -p quark-cli -- --cookie-file cookie.txt ls --fid 0
cargo run -p quark-cli -- --cookie-file cookie.txt upload --file ./acceptance.bin --parent 0
cargo run -p quark-cli -- --cookie-file cookie.txt download --fid <fid> --output ./acceptance.download
```

检查项：

- `/member` 返回会员类型、容量；昵称和头像由 `/account/info` 补充。
- 目录列表与网页端一致。
- 上传后网页端能看到文件，大小一致。
- 下载文件与上传文件 SHA256 一致。
- 上传/下载中断后重启任务能继续。

## 已知限制

- 未实现分享链接、在线播放、FUSE 挂载、自动同步。
- 凭据目前使用 0600 本地文件，尚未接入 Secret Service；生产分发建议换成系统 keyring。
- 夸克没有公开的个人开发者 OAuth，当前使用网页端 Cookie 接口；接口变更时需要更新 `quark-protocol`。
- 默认并发较低（2 个任务、单文件 3 连接），避免触发风控。

## 安全

- Cookie 只发送给夸克自己的域名，不上传到第三方，不进入前端状态，也不写入日志。
- 前端无法读取凭据，所有 API 调用在 Rust 侧完成。
- 运行数据目录权限为 `0700`，数据库与凭据文件权限为 `0600`。
- Linux 默认数据目录：`~/.local/share/cn.quarkdrive.desktop`。
- 当前版本默认不收集任何远程数据；提交 Issue 前请自行脱敏日志、路径和账号信息。

## 开源与许可

- 本项目使用 [MIT License](LICENSE)。
- 发布二进制时请一并附带 Rust 与 npm 依赖的第三方许可证清单。
- “夸克”及相关商标归其权利人所有，本项目名称仅用于说明兼容性。
- 安全问题请按 [SECURITY.md](SECURITY.md) 私下报告，不要公开粘贴真实 Cookie。
