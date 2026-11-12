import type { FileItem, ListResult, Transfer } from "./types";

export const PRIVACY_PAGE_SIZE = 100;

export function privacyUploadDoneCount(
  transfers: Transfer[],
  accountId: string,
  parentFid: string,
): number {
  return transfers.filter(
    (transfer) =>
      transfer.id.startsWith("privacy:") &&
      transfer.accountId === accountId &&
      transfer.kind === "upload" &&
      transfer.remoteParentFid === parentFid &&
      transfer.status === "done",
  ).length;
}

export interface PrivacyBackend {
  privacyRegistered: (accountId: string) => Promise<boolean>;
  privacyUnlock: (accountId: string, password: string) => Promise<void>;
  privacyListDir: (accountId: string, parentFid: string, page: number, size: number) => Promise<ListResult>;
  privacyLock: (accountId: string) => Promise<void>;
}

export interface PrivacyCrumb {
  fid: string;
  name: string;
}

export interface PrivacyState {
  phase: "checking" | "locked" | "unlocking" | "browsing" | "locking" | "unavailable" | "failure";
  crumbs: PrivacyCrumb[];
  entries: FileItem[];
  page: number;
  total: number;
  loading: boolean;
  message: string;
}

export function initialPrivacyState(): PrivacyState {
  return {
    phase: "checking",
    crumbs: [{ fid: "2", name: "隐私空间" }],
    entries: [],
    page: 1,
    total: 0,
    loading: false,
    message: "",
  };
}

export class PrivacyLockQueue {
  private pending = new Map<string, Promise<void>>();

  lock(accountId: string, action: () => Promise<void>): Promise<void> {
    const previous = this.pending.get(accountId);
    const operation = previous
      ? previous.catch(() => undefined).then(action)
      : action();
    this.pending.set(accountId, operation);
    const remove = () => {
      if (this.pending.get(accountId) === operation) this.pending.delete(accountId);
    };
    void operation.then(remove, remove);
    return operation;
  }
}

const privacyLocks = new PrivacyLockQueue();

export class PrivacySession {
  private state = initialPrivacyState();
  private generation = 0;
  private disposed = false;
  private pendingUnlock: Promise<void> | undefined;
  private listener: ((state: PrivacyState) => void) | undefined;

  constructor(
    private readonly accountId: string,
    private readonly api: PrivacyBackend,
    private readonly locks: PrivacyLockQueue = privacyLocks,
  ) {}

  subscribe(listener: (state: PrivacyState) => void): () => void {
    this.listener = listener;
    listener(this.state);
    return () => {
      if (this.listener === listener) this.listener = undefined;
    };
  }

  private publish(state: PrivacyState) {
    this.state = state;
    if (!this.disposed) this.listener?.(state);
  }

  private current(generation: number) {
    return !this.disposed && generation === this.generation;
  }

  private lockBackend() {
    const pendingUnlock = this.pendingUnlock;
    return this.locks.lock(this.accountId, async () => {
      if (pendingUnlock) {
        await Promise.all([
          this.api.privacyLock(this.accountId).catch(() => undefined),
          pendingUnlock.catch(() => undefined),
        ]);
      }
      await this.api.privacyLock(this.accountId);
    });
  }

  async initialize(): Promise<void> {
    if (this.disposed) return;
    const generation = ++this.generation;
    this.publish(initialPrivacyState());
    try {
      await this.lockBackend();
      if (!this.current(generation)) return;
      const registered = await this.api.privacyRegistered(this.accountId);
      if (!this.current(generation)) return;
      this.publish({
        ...initialPrivacyState(),
        phase: registered ? "locked" : "unavailable",
      });
    } catch {
      if (!this.current(generation)) return;
      this.publish({
        ...initialPrivacyState(),
        phase: "failure",
        message: "无法建立安全的隐私会话。请检查网络与账号登录状态后重试。",
      });
    }
  }

  async unlock(password: string): Promise<void> {
    if (this.disposed || this.state.phase !== "locked" || password.length === 0) return;
    const generation = ++this.generation;
    this.publish({ ...initialPrivacyState(), phase: "unlocking" });
    try {
      const request = this.api.privacyUnlock(this.accountId, password);
      password = "";
      this.pendingUnlock = request;
      try {
        await request;
      } finally {
        if (this.pendingUnlock === request) this.pendingUnlock = undefined;
      }
      if (!this.current(generation)) return;
      this.publish({ ...initialPrivacyState(), phase: "browsing" });
      await this.load(this.state.crumbs, 1);
    } catch {
      if (!this.current(generation)) return;
      await this.lock("解锁未成功。请确认隐私密码或稍后重试，请勿连续尝试密码。");
    }
  }

  async load(crumbs: PrivacyCrumb[], page: number): Promise<void> {
    if (this.disposed || this.state.phase !== "browsing" || this.state.loading) return;
    if (!Number.isInteger(page) || page < 1 || crumbs.length === 0) return;
    const generation = ++this.generation;
    this.publish({ ...this.state, crumbs, page, entries: [], total: 0, loading: true, message: "" });
    try {
      const result = await this.api.privacyListDir(
        this.accountId,
        crumbs[crumbs.length - 1].fid,
        page,
        PRIVACY_PAGE_SIZE,
      );
      if (!this.current(generation)) return;
      this.publish({ ...this.state, entries: result.entries, total: result.total, loading: false });
    } catch {
      if (!this.current(generation)) return;
      await this.lock("无法读取隐私目录，已清除本地显示并锁定。请稍后再试。");
    }
  }

  async lock(message = ""): Promise<void> {
    if (this.disposed) return;
    const generation = ++this.generation;
    this.publish({ ...initialPrivacyState(), phase: "locking" });
    try {
      await this.lockBackend();
      if (!this.current(generation)) return;
      this.publish({ ...initialPrivacyState(), phase: "locked", message });
    } catch {
      if (!this.current(generation)) return;
      this.publish({
        ...initialPrivacyState(),
        phase: "failure",
        message: "本地显示已清除，但无法确认后端锁定。请退出软件后重试。",
      });
    }
  }

  forget(fids: string[]): void {
    if (this.disposed || this.state.phase !== "browsing" || fids.length === 0) return;
    const removedFids = new Set(fids);
    const entries = this.state.entries.filter((entry) => !removedFids.has(entry.fid));
    if (entries.length === this.state.entries.length) return;
    this.publish({
      ...this.state,
      entries,
      total: Math.max(0, this.state.total - (this.state.entries.length - entries.length)),
    });
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation += 1;
    this.state = initialPrivacyState();
    this.listener = undefined;
    void this.lockBackend().catch(() => undefined);
  }
}
