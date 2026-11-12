import type {
  Account,
  Bootstrap,
  CoreEvent,
  DownloadItem,
  FileItem,
  ListResult,
  QrLoginPoll,
  QrLoginStart,
  RecycleItem,
  RecycleListResult,
  Settings,
  Transfer,
} from "./types";

type Handler = (event: CoreEvent) => void;

const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const isDesktop = isTauri;

export async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isTauri) {
    return mockInvoke(command, args) as Promise<T>;
  }
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

export async function subscribeEvents(handler: Handler): Promise<() => void> {
  if (!isTauri) {
    return mockSubscribe(handler);
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<CoreEvent>("core://event", (event) =>
    handler(event.payload),
  );
  return unlisten;
}

export const backend = {
  bootstrap: () => invoke<Bootstrap>("get_bootstrap"),
  startQrLogin: () => invoke<QrLoginStart>("start_qr_login"),
  pollQrLogin: (sessionId: string) =>
    invoke<QrLoginPoll>("poll_qr_login", { sessionId }),
  cancelQrLogin: (sessionId: string) =>
    invoke<void>("cancel_qr_login", { sessionId }),
  importCookie: (cookie: string) =>
    invoke<Account>("import_cookie", { cookie }),
  logout: (accountId: string) => invoke<void>("logout", { accountId }),
  refreshAccount: (accountId: string) =>
    invoke<Account>("refresh_account", { accountId }),
  listDir: (accountId: string, parentFid: string, page = 1, size = 200) =>
    invoke<ListResult>("list_dir", { accountId, parentFid, page, size }),
  privacyRegistered: (accountId: string) =>
    invoke<boolean>("privacy_registered", { accountId }),
  privacyUnlock: (accountId: string, password: string) =>
    invoke<void>("privacy_unlock", { accountId, password }),
  privacyListDir: (accountId: string, parentFid: string, page: number, size: number) =>
    invoke<ListResult>("privacy_list_dir", { accountId, parentFid, page, size }),
  privacyLock: (accountId: string) =>
    invoke<void>("privacy_lock", { accountId }),
  privacyMoveFiles: (accountId: string, fids: string[], parentFid: string) =>
    invoke<void>("privacy_move_files", { accountId, fids, parentFid }),
  privacyEnqueueUpload: (
    accountId: string,
    paths: string[],
    parentFid: string,
  ) =>
    invoke<string[]>("privacy_enqueue_upload", {
      accountId,
      paths,
      parentFid,
    }),
  privacyEnqueueDownload: (
    accountId: string,
    items: DownloadItem[],
    destDir?: string,
  ) =>
    invoke<string[]>("privacy_enqueue_download", {
      accountId,
      items,
      destDir: destDir ?? null,
    }),
  cachedFiles: (accountId: string, parentFid: string) =>
    invoke<FileItem[]>("cached_files", { accountId, parentFid }),
  mkdir: (accountId: string, parentFid: string, name: string) =>
    invoke<string>("mkdir", { accountId, parentFid, name }),
  rename: (accountId: string, fid: string, name: string) =>
    invoke<void>("rename", { accountId, fid, name }),
  moveFiles: (accountId: string, fids: string[], parentFid: string) =>
    invoke<void>("move_files", { accountId, fids, parentFid }),
  deleteFiles: (accountId: string, fids: string[], permanent = false) =>
    invoke<void>("delete_files", { accountId, fids, permanent }),
  listRecycle: (accountId: string, page = 1, size = 100) =>
    invoke<RecycleListResult>("list_recycle", { accountId, page, size }),
  restoreRecycle: (accountId: string, recordIds: string[]) =>
    invoke<void>("restore_recycle", { accountId, recordIds }),
  deleteRecycle: (accountId: string, recordIds: string[]) =>
    invoke<void>("delete_recycle", { accountId, recordIds }),
  clearRecycle: (accountId: string) =>
    invoke<number>("clear_recycle", { accountId }),
  enqueueUpload: (accountId: string, paths: string[], parentFid: string) =>
    invoke<string[]>("enqueue_upload", { accountId, paths, parentFid }),
  enqueueDownload: (
    accountId: string,
    items: DownloadItem[],
    destDir?: string,
  ) =>
    invoke<string[]>("enqueue_download", {
      accountId,
      items,
      destDir: destDir ?? null,
    }),
  listTransfers: (limit = 200) =>
    invoke<Transfer[]>("list_transfers", { limit }),
  pauseTransfer: (id: string) => invoke<void>("pause_transfer", { id }),
  resumeTransfer: (id: string) => invoke<void>("resume_transfer", { id }),
  cancelTransfer: (id: string) => invoke<void>("cancel_transfer", { id }),
  retryTransfer: (id: string) => invoke<void>("retry_transfer", { id }),
  removeTransfer: (id: string) => invoke<void>("remove_transfer", { id }),
  clearFinished: () => invoke<void>("clear_finished_transfers"),
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (patch: Partial<Settings>) =>
    invoke<Settings>("update_settings", {
      downloadDir: patch.downloadDir ?? null,
      concurrency: patch.concurrency ?? null,
      maxSpeedBps: patch.maxSpeedBps ?? null,
      theme: patch.theme ?? null,
    }),
  pickFiles: () => invoke<string[]>("pick_files"),
  pickFolder: () => invoke<string | null>("pick_folder"),
  openPath: (path: string) => invoke<void>("open_path", { path }),
};

// ---------------------------------------------------------------------------
// Browser demo backend: lets the UI be developed and visually verified without
// a Tauri runtime or a Quark account.
// ---------------------------------------------------------------------------

const demoAccount: Account = {
  id: "demo",
  uid: "10000001",
  nickname: "演示用户",
  avatarUrl: "",
  vipType: "SUPER_VIP",
  vipExpireAt: Date.now() + 365 * 24 * 60 * 60 * 1000,
  isVip: true,
  totalSpace: 300 * 1024 ** 3,
  usedSpace: 126 * 1024 ** 3,
  lastCheckAt: Date.now(),
};

const demoFiles: FileItem[] = [
  folder("f-docs", "文档", 0),
  folder("f-movies", "影视资源", 0),
  folder("f-photos", "照片备份", 0),
  folder("f-work", "工作资料", 0),
  file("f-1", "夸克网盘 Linux 客户端方案.pdf", 4.8 * 1024 ** 2, 2),
  file("f-2", "Ubuntu 24.04 配置笔记.md", 38 * 1024, 2),
  file("f-3", "产品设计稿-终版.fig", 86 * 1024 ** 2, 2),
  file("f-4", "星际穿越.2014.2160p.mkv", 22.4 * 1024 ** 3, 1),
  file("f-5", "旅行照片-2026.zip", 3.2 * 1024 ** 3, 1),
  file("f-6", "团队周报-第44周.docx", 1.2 * 1024 ** 2, 2),
];

function folder(fid: string, name: string, updatedAt: number): FileItem {
  return {
    fid,
    parentFid: "0",
    name,
    isDir: true,
    size: 0,
    category: 0,
    formatType: "",
    updatedAt: updatedAt || Date.now() - 86400000,
    thumbnail: "",
  };
}

function file(fid: string, name: string, size: number, category: number): FileItem {
  return {
    fid,
    parentFid: "0",
    name,
    isDir: false,
    size,
    category,
    formatType: name.split(".").pop() || "",
    updatedAt: Date.now() - Math.floor(Math.random() * 20) * 86400000,
    thumbnail: "",
  };
}

let demoTransfers: Transfer[] = [
  {
    id: "t-demo-1",
    accountId: "demo",
    kind: "upload",
    status: "running",
    phase: "uploading",
    fid: null,
    localPath: "/home/demo/Videos/旅行记录.mp4",
    remoteName: "旅行记录.mp4",
    remoteParentFid: "0",
    totalBytes: 2.8 * 1024 ** 3,
    doneBytes: 1.42 * 1024 ** 3,
    speedBps: 18.6 * 1024 ** 2,
    createdAt: Date.now() - 65000,
    updatedAt: Date.now(),
  },
  {
    id: "t-demo-2",
    accountId: "demo",
    kind: "download",
    status: "running",
    phase: "downloading",
    fid: "f-4",
    localPath: "/home/demo/Downloads/星际穿越.2014.2160p.mkv",
    remoteName: "星际穿越.2014.2160p.mkv",
    remoteParentFid: "0",
    totalBytes: 22.4 * 1024 ** 3,
    doneBytes: 8.1 * 1024 ** 3,
    speedBps: 42.3 * 1024 ** 2,
    createdAt: Date.now() - 180000,
    updatedAt: Date.now(),
  },
  {
    id: "t-demo-3",
    accountId: "demo",
    kind: "upload",
    status: "done",
    phase: "done",
    fid: null,
    localPath: "/home/demo/Documents/方案.pdf",
    remoteName: "方案.pdf",
    remoteParentFid: "0",
    totalBytes: 12.4 * 1024 ** 2,
    doneBytes: 12.4 * 1024 ** 2,
    speedBps: 0,
    createdAt: Date.now() - 3600000,
    updatedAt: Date.now() - 3550000,
  },
];

const demoListeners = new Set<Handler>();
let demoTimer: number | undefined;

function emitDemo(event: CoreEvent) {
  demoListeners.forEach((listener) => listener(event));
}

function ensureDemoTimer() {
  if (demoTimer) return;
  demoTimer = window.setInterval(() => {
    let changed = false;
    demoTransfers = demoTransfers.map((transfer) => {
      if (transfer.status !== "running") return transfer;
      const delta = Math.max(512 * 1024, transfer.speedBps * 0.8);
      const done = Math.min(transfer.totalBytes, transfer.doneBytes + delta);
      changed = true;
      return {
        ...transfer,
        doneBytes: done,
        status: done >= transfer.totalBytes ? "done" : transfer.status,
        phase: done >= transfer.totalBytes ? "done" : transfer.phase,
        speedBps: done >= transfer.totalBytes ? 0 : transfer.speedBps,
        updatedAt: Date.now(),
      };
    });
    if (changed) {
      demoTransfers.forEach((transfer) => {
        if (transfer.status === "running") {
          emitDemo({
            type: "transferProgress",
            id: transfer.id,
            status: transfer.status,
            phase: transfer.phase,
            doneBytes: transfer.doneBytes,
            totalBytes: transfer.totalBytes,
            speedBps: transfer.speedBps,
          });
        }
      });
    }
  }, 900);
}

function mockSubscribe(handler: Handler): () => void {
  demoListeners.add(handler);
  ensureDemoTimer();
  return () => demoListeners.delete(handler);
}

const demoState = {
  loggedIn: false,
  settings: {
    downloadDir: "/home/demo/Downloads/QuarkDrive",
    concurrency: 2,
    maxSpeedBps: 0,
    theme: "system",
  } as Settings,
  files: demoFiles,
  recycle: [
    {
      recordId: "r-demo-1",
      fid: "f-recycle-1",
      name: "2025-旧项目备份",
      isDir: true,
      size: 0,
      category: 0,
      formatType: "",
      parentFid: "0",
      deletedAt: Date.now() - 3 * 86400000,
      updatedAt: Date.now() - 20 * 86400000,
    },
    {
      recordId: "r-demo-2",
      fid: "f-recycle-2",
      name: "废弃草稿.txt",
      isDir: false,
      size: 2048,
      category: 0,
      formatType: "txt",
      parentFid: "0",
      deletedAt: Date.now() - 9 * 86400000,
      updatedAt: Date.now() - 30 * 86400000,
    },
  ] as RecycleItem[],
};

async function mockInvoke(
  command: string,
  args?: Record<string, unknown>,
): Promise<unknown> {
  await new Promise((resolve) => setTimeout(resolve, 120));
  switch (command) {
    case "get_bootstrap":
      return {
        accounts: demoState.loggedIn ? [demoAccount] : [],
        settings: demoState.settings,
        transfers: demoTransfers,
        defaultParentFid: "0",
        dataDir: "/home/demo/.local/share/cn.quarkdrive.desktop",
      } satisfies Bootstrap;
    case "start_qr_login":
      return {
        sessionId: `demo-${Date.now()}`,
        qrUrl: "https://su.quark.cn/4_eMHBJ?token=demo-qr-token",
        expiresAt: Date.now() + 180_000,
      } satisfies QrLoginStart;
    case "poll_qr_login":
      return { state: "waiting" } satisfies QrLoginPoll;
    case "cancel_qr_login":
      return undefined;
    case "import_cookie":
      demoState.loggedIn = true;
      return demoAccount;
    case "refresh_account":
      return demoAccount;
    case "logout":
      demoState.loggedIn = false;
      return undefined;
    case "privacy_lock":
      return undefined;
    case "privacy_registered":
    case "privacy_unlock":
    case "privacy_list_dir":
    case "privacy_move_files":
    case "privacy_enqueue_upload":
    case "privacy_enqueue_download":
      throw new Error("隐私空间仅可在桌面客户端连接真实账号使用，浏览器演示不模拟解锁成功");
    case "list_dir": {
      const parent = String(args?.parentFid || "0");
      const entries =
        parent === "0"
          ? demoState.files
          : demoState.files.filter((item) => item.parentFid === parent);
      return {
        entries,
        total: entries.length,
        page: 1,
        size: 200,
      } satisfies ListResult;
    }
    case "mkdir": {
      const name = String(args?.name || "新建文件夹");
      const parentFid = String(args?.parentFid || "0");
      const item = folder(`f-${Date.now()}`, name, Date.now());
      item.parentFid = parentFid;
      demoState.files = [item, ...demoState.files];
      return item.fid;
    }
    case "rename": {
      const fid = String(args?.fid);
      const name = String(args?.name);
      demoState.files = demoState.files.map((item) =>
        item.fid === fid ? { ...item, name } : item,
      );
      return undefined;
    }
    case "move_files": {
      const fids = new Set((args?.fids as string[]) || []);
      const parentFid = String(args?.parentFid || "0");
      demoState.files = demoState.files.map((item) =>
        fids.has(item.fid) ? { ...item, parentFid } : item,
      );
      return undefined;
    }
    case "delete_files": {
      const fids = new Set((args?.fids as string[]) || []);
      if (!args?.permanent) {
        const removed = demoState.files.filter((item) => fids.has(item.fid));
        demoState.recycle = [
          ...removed.map((item) => ({
            recordId: `r-${Date.now()}-${item.fid}`,
            fid: item.fid,
            name: item.name,
            isDir: item.isDir,
            size: item.size,
            category: item.category,
            formatType: item.formatType,
            parentFid: item.parentFid,
            deletedAt: Date.now(),
            updatedAt: item.updatedAt,
          })),
          ...demoState.recycle,
        ];
      }
      demoState.files = demoState.files.filter((item) => !fids.has(item.fid));
      return undefined;
    }
    case "list_recycle": {
      const page = Number(args?.page || 1);
      const size = Number(args?.size || 100);
      const start = (page - 1) * size;
      return {
        entries: demoState.recycle.slice(start, start + size),
        total: demoState.recycle.length,
        page,
        size,
      } satisfies RecycleListResult;
    }
    case "restore_recycle": {
      const ids = new Set((args?.recordIds as string[]) || []);
      const restored = demoState.recycle.filter((item) => ids.has(item.recordId));
      demoState.recycle = demoState.recycle.filter((item) => !ids.has(item.recordId));
      demoState.files = [
        ...restored.map((item) => ({
          fid: item.fid,
          parentFid: item.parentFid || "0",
          name: item.name,
          isDir: item.isDir,
          size: item.size,
          category: item.category,
          formatType: item.formatType,
          updatedAt: item.updatedAt,
          thumbnail: "",
        })),
        ...demoState.files,
      ];
      return undefined;
    }
    case "delete_recycle": {
      const ids = new Set((args?.recordIds as string[]) || []);
      demoState.recycle = demoState.recycle.filter((item) => !ids.has(item.recordId));
      return undefined;
    }
    case "clear_recycle": {
      const count = demoState.recycle.length;
      demoState.recycle = [];
      return count;
    }
    case "enqueue_upload":
    case "enqueue_download": {
      const now = Date.now();
      const kind = command === "enqueue_upload" ? "upload" : "download";
      const transfer: Transfer = {
        id: `t-${now}`,
        accountId: "demo",
        kind,
        status: "running",
        phase: kind === "upload" ? "uploading" : "downloading",
        fid: null,
        localPath:
          kind === "upload"
            ? String((args?.paths as string[])?.[0] || "/home/demo/file")
            : "/home/demo/Downloads/file",
        remoteName:
          kind === "upload"
            ? String((args?.paths as string[])?.[0] || "file").split("/").pop()!
            : "下载文件",
        remoteParentFid: "0",
        totalBytes: 1.2 * 1024 ** 3,
        doneBytes: 0,
        speedBps: 22 * 1024 ** 2,
        createdAt: now,
        updatedAt: now,
      };
      demoTransfers = [transfer, ...demoTransfers];
      emitDemo({ type: "transferChanged", transfer });
      return [transfer.id];
    }
    case "list_transfers":
      return demoTransfers;
    case "pause_transfer": {
      const id = String(args?.id);
      demoTransfers = demoTransfers.map((item) =>
        item.id === id
          ? { ...item, status: "paused", phase: "paused", speedBps: 0 }
          : item,
      );
      const transfer = demoTransfers.find((item) => item.id === id);
      if (transfer) emitDemo({ type: "transferChanged", transfer });
      return undefined;
    }
    case "resume_transfer":
    case "retry_transfer": {
      const id = String(args?.id);
      demoTransfers = demoTransfers.map((item) =>
        item.id === id
          ? { ...item, status: "running", phase: item.kind === "upload" ? "uploading" : "downloading", speedBps: 20 * 1024 ** 2 }
          : item,
      );
      const transfer = demoTransfers.find((item) => item.id === id);
      if (transfer) emitDemo({ type: "transferChanged", transfer });
      return undefined;
    }
    case "cancel_transfer": {
      const id = String(args?.id);
      demoTransfers = demoTransfers.map((item) =>
        item.id === id
          ? { ...item, status: "canceled", phase: "canceled", speedBps: 0 }
          : item,
      );
      const transfer = demoTransfers.find((item) => item.id === id);
      if (transfer) emitDemo({ type: "transferChanged", transfer });
      return undefined;
    }
    case "remove_transfer": {
      const id = String(args?.id);
      demoTransfers = demoTransfers.filter((item) => item.id !== id);
      return undefined;
    }
    case "clear_finished_transfers":
      demoTransfers = demoTransfers.filter(
        (item) => !["done", "canceled", "failed"].includes(item.status),
      );
      return undefined;
    case "get_settings":
      return demoState.settings;
    case "update_settings": {
      demoState.settings = {
        ...demoState.settings,
        ...(args?.downloadDir ? { downloadDir: String(args.downloadDir) } : {}),
        ...(args?.concurrency ? { concurrency: Number(args.concurrency) } : {}),
        ...(args?.maxSpeedBps !== null && args?.maxSpeedBps !== undefined
          ? { maxSpeedBps: Number(args.maxSpeedBps) }
          : {}),
        ...(args?.theme ? { theme: String(args.theme) } : {}),
      };
      return demoState.settings;
    }
    case "pick_files":
      return ["/home/demo/Videos/旅行记录.mp4"];
    case "pick_folder":
      return "/home/demo/Downloads/QuarkDrive";
    case "open_path":
      return undefined;
    default:
      return undefined;
  }
}
