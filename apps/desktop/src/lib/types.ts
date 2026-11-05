export interface Account {
  id: string;
  uid: string;
  nickname: string;
  avatarUrl: string;
  vipType: string;
  vipExpireAt?: number | null;
  isVip: boolean;
  totalSpace: number;
  usedSpace: number;
  lastCheckAt?: number | null;
}

export interface FileItem {
  fid: string;
  parentFid: string;
  name: string;
  isDir: boolean;
  size: number;
  category: number;
  formatType: string;
  updatedAt: number;
  thumbnail: string;
}

export interface ListResult {
  entries: FileItem[];
  total: number;
  page: number;
  size: number;
}

export interface RecycleItem {
  recordId: string;
  fid: string;
  name: string;
  isDir: boolean;
  size: number;
  category: number;
  formatType: string;
  parentFid: string;
  deletedAt: number;
  updatedAt: number;
}

export interface RecycleListResult {
  entries: RecycleItem[];
  total: number;
  page: number;
  size: number;
}

export interface Transfer {
  id: string;
  accountId: string;
  kind: "upload" | "download";
  status: string;
  phase: string;
  fid?: string | null;
  localPath: string;
  remoteName: string;
  remoteParentFid: string;
  totalBytes: number;
  doneBytes: number;
  speedBps: number;
  errorCode?: string | null;
  errorMessage?: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface Settings {
  downloadDir: string;
  concurrency: number;
  maxSpeedBps: number;
  theme: "light" | "dark" | "system" | string;
}

export interface Bootstrap {
  accounts: Account[];
  settings: Settings;
  transfers: Transfer[];
  defaultParentFid: string;
  dataDir: string;
}

export interface DownloadItem {
  fid: string;
  name: string;
  size: number;
  isDir: boolean;
}

export interface QrLoginStart {
  sessionId: string;
  qrUrl: string;
  expiresAt: number;
}

export interface QrLoginPoll {
  state: "waiting" | "success" | "expired";
  expiresAt?: number;
  account?: Account;
  message?: string;
}

export type CoreEvent =
  | { type: "accountsChanged"; accounts: Account[] }
  | {
      type: "transferProgress";
      id: string;
      status: string;
      phase: string;
      doneBytes: number;
      totalBytes: number;
      speedBps: number;
    }
  | { type: "transferChanged"; transfer: Transfer }
  | { type: "toast"; level: string; message: string }
  | { type: "loginState"; state: string; message: string };
