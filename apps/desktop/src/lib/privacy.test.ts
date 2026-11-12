import { describe, expect, it, vi } from "vitest";
import {
  initialPrivacyState,
  PrivacyLockQueue,
  PrivacySession,
  privacyUploadDoneCount,
  PRIVACY_PAGE_SIZE,
  type PrivacyBackend,
  type PrivacyState,
} from "./privacy";
import type { ListResult, Transfer } from "./types";

function deferred<Value>() {
  let resolve!: (value: Value) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<Value>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function listing(parentFid = "2", page = 1): ListResult {
  return {
    entries: [{
      fid: "private-folder",
      parentFid,
      name: "synthetic-private-name",
      isDir: true,
      size: 0,
      category: 0,
      formatType: "",
      updatedAt: 0,
      thumbnail: "",
    }],
    page,
    size: PRIVACY_PAGE_SIZE,
    total: 201,
  };
}

function transfer(overrides: Partial<Transfer> = {}): Transfer {
  return {
    id: "privacy:upload-one",
    accountId: "account-one",
    kind: "upload",
    status: "done",
    phase: "done",
    localPath: "/tmp/local-file",
    remoteName: "remote-file",
    remoteParentFid: "2",
    totalBytes: 1,
    doneBytes: 1,
    speedBps: 0,
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

function setup(overrides: Partial<PrivacyBackend> = {}, locks = new PrivacyLockQueue(), accountId = "account-one") {
  const api = {
    privacyRegistered: vi.fn<PrivacyBackend["privacyRegistered"]>().mockResolvedValue(true),
    privacyUnlock: vi.fn<PrivacyBackend["privacyUnlock"]>().mockResolvedValue(undefined),
    privacyListDir: vi.fn<PrivacyBackend["privacyListDir"]>().mockResolvedValue(listing()),
    privacyLock: vi.fn<PrivacyBackend["privacyLock"]>().mockResolvedValue(undefined),
    ...overrides,
  };
  const session = new PrivacySession(accountId, api, locks);
  let state = initialPrivacyState();
  const history: PrivacyState[] = [];
  const listener = vi.fn((next: PrivacyState) => {
    state = next;
    history.push(next);
  });
  session.subscribe(listener);
  return { api, session, getState: () => state, history, listener };
}

describe("privacy session", () => {
  it("counts only completed privacy uploads in the current directory", () => {
    const transfers = [
      transfer(),
      transfer({ id: "privacy:upload-two" }),
      transfer({ id: "privacy:running", status: "running" }),
      transfer({ id: "privacy:failed", status: "failed" }),
      transfer({ id: "privacy:other-parent", remoteParentFid: "folder-two" }),
      transfer({ id: "privacy:other-account", accountId: "account-two" }),
      transfer({ id: "normal-upload", accountId: "account-one" }),
      transfer({ id: "privacy:download", kind: "download" }),
    ];
    expect(privacyUploadDoneCount(transfers, "account-one", "2")).toBe(2);
  });

  it("locks before checking registration and never attempts a password automatically", async () => {
    const locked = deferred<void>();
    const test = setup({ privacyLock: vi.fn().mockReturnValue(locked.promise) });
    const initialization = test.session.initialize();
    expect(test.getState().phase).toBe("checking");
    expect(test.api.privacyRegistered).not.toHaveBeenCalled();
    expect(test.api.privacyUnlock).not.toHaveBeenCalled();
    locked.resolve();
    await initialization;
    expect(test.api.privacyRegistered).toHaveBeenCalledWith("account-one");
    expect(test.getState().phase).toBe("locked");
  });

  it("does not unlock or register an unregistered privacy space", async () => {
    const test = setup({ privacyRegistered: vi.fn().mockResolvedValue(false) });
    await test.session.initialize();
    await test.session.unlock("synthetic-password");
    expect(test.getState().phase).toBe("unavailable");
    expect(test.api.privacyUnlock).not.toHaveBeenCalled();
  });

  it("preserves password whitespace without storing it in state", async () => {
    const test = setup();
    await test.session.initialize();
    await test.session.unlock("  synthetic-password  ");
    expect(test.api.privacyUnlock).toHaveBeenCalledTimes(1);
    expect(test.api.privacyUnlock).toHaveBeenCalledWith("account-one", "  synthetic-password  ");
    expect(test.api.privacyListDir).toHaveBeenCalledTimes(1);
    expect(test.api.privacyListDir).toHaveBeenCalledWith("account-one", "2", 1, 100);
    expect(test.getState().phase).toBe("browsing");
    expect(JSON.stringify(test.history)).not.toContain("synthetic-password");
  });

  it("rejects an empty password and duplicate unlock submissions", async () => {
    const unlock = deferred<void>();
    const test = setup({ privacyUnlock: vi.fn().mockReturnValue(unlock.promise) });
    await test.session.initialize();
    await test.session.unlock("");
    expect(test.api.privacyUnlock).not.toHaveBeenCalled();
    const submitted = test.session.unlock("first-password");
    await test.session.unlock("second-password");
    expect(test.api.privacyUnlock).toHaveBeenCalledTimes(1);
    unlock.resolve();
    await submitted;
  });

  it("loads child directories and pages without merging or retaining old entries", async () => {
    const nextPage = deferred<ListResult>();
    const test = setup();
    await test.session.initialize();
    await test.session.unlock("synthetic-password");
    const crumbs = [...test.getState().crumbs, { fid: "private-folder", name: "synthetic-private-name" }];
    await test.session.load(crumbs, 1);
    expect(test.api.privacyListDir).toHaveBeenLastCalledWith("account-one", "private-folder", 1, 100);
    vi.mocked(test.api.privacyListDir).mockReturnValueOnce(nextPage.promise);
    const loading = test.session.load(crumbs, 2);
    expect(test.getState().entries).toEqual([]);
    expect(test.getState().loading).toBe(true);
    expect(test.api.privacyListDir).toHaveBeenLastCalledWith("account-one", "private-folder", 2, 100);
    nextPage.resolve({ ...listing("private-folder", 2), entries: [] });
    await loading;
    expect(test.getState().entries).toEqual([]);
    expect(test.getState().page).toBe(2);
    expect(test.getState().total).toBe(201);
  });

  it("removes moved entries immediately without waiting for a stale refetch", async () => {
    const test = setup();
    await test.session.initialize();
    await test.session.unlock("synthetic-password");
    const before = test.getState();
    test.session.forget(["private-folder"]);
    expect(test.getState().entries).toEqual([]);
    expect(test.getState().total).toBe(before.total - 1);
    const afterFirstForget = test.getState();
    test.session.forget(["private-folder"]);
    expect(test.getState()).toEqual(afterFirstForget);
  });

  it("clears names and breadcrumbs immediately on lock and rejects late listing results", async () => {
    const response = deferred<ListResult>();
    const lock = deferred<void>();
    const test = setup();
    await test.session.initialize();
    await test.session.unlock("synthetic-password");
    vi.mocked(test.api.privacyListDir).mockReturnValueOnce(response.promise);
    const loading = test.session.load([
      ...test.getState().crumbs,
      { fid: "private-folder", name: "synthetic-private-name" },
    ], 1);
    vi.mocked(test.api.privacyLock).mockReturnValueOnce(lock.promise);
    const locking = test.session.lock();
    expect(test.getState().phase).toBe("locking");
    expect(test.getState().entries).toEqual([]);
    expect(JSON.stringify(test.getState())).not.toContain("synthetic-private-name");
    response.resolve(listing("private-folder"));
    await loading;
    expect(test.getState().entries).toEqual([]);
    lock.resolve();
    await locking;
    expect(test.getState().phase).toBe("locked");
  });

  it("does not let a late unlock reopen a locked session", async () => {
    const response = deferred<void>();
    const test = setup({ privacyUnlock: vi.fn().mockReturnValue(response.promise) });
    await test.session.initialize();
    const unlocking = test.session.unlock("synthetic-password");
    const locking = test.session.lock();
    expect(test.getState().phase).toBe("locking");
    response.resolve();
    await unlocking;
    await locking;
    expect(test.getState().phase).toBe("locked");
    expect(test.api.privacyListDir).not.toHaveBeenCalled();
    expect(test.api.privacyLock).toHaveBeenCalledTimes(3);
  });

  it("does not retry passwords or render raw unlock failures", async () => {
    const test = setup({ privacyUnlock: vi.fn().mockRejectedValue(new Error("token=private-error-content")) });
    await test.session.initialize();
    await test.session.unlock("synthetic-password");
    expect(test.api.privacyUnlock).toHaveBeenCalledTimes(1);
    expect(test.getState().phase).toBe("locked");
    expect(test.getState().message).toContain("解锁未成功");
    expect(JSON.stringify(test.history)).not.toContain("private-error-content");
    expect(test.api.privacyLock).toHaveBeenCalledTimes(2);
  });

  it("locks and clears prior names on a directory failure without exposing server content", async () => {
    const test = setup();
    await test.session.initialize();
    await test.session.unlock("synthetic-password");
    vi.mocked(test.api.privacyListDir).mockRejectedValueOnce(new Error("private-server-content"));
    await test.session.load(test.getState().crumbs, 2);
    expect(test.getState().phase).toBe("locked");
    expect(test.getState().message).toContain("无法读取隐私目录");
    expect(test.getState().entries).toEqual([]);
    expect(JSON.stringify(test.getState())).not.toContain("synthetic-private-name");
    expect(JSON.stringify(test.history)).not.toContain("private-server-content");
  });

  it("fails closed when lock confirmation fails", async () => {
    const test = setup({ privacyLock: vi.fn().mockRejectedValue(new Error("private-content")) });
    await test.session.initialize();
    expect(test.getState().phase).toBe("failure");
    await test.session.unlock("synthetic-password");
    expect(test.api.privacyUnlock).not.toHaveBeenCalled();
    expect(test.api.privacyRegistered).not.toHaveBeenCalled();
    expect(JSON.stringify(test.history)).not.toContain("private-content");
  });

  it("locks once on dispose and does not deliver pending private responses to the UI", async () => {
    const response = deferred<ListResult>();
    const test = setup({ privacyListDir: vi.fn().mockReturnValue(response.promise) });
    await test.session.initialize();
    const unlocking = test.session.unlock("synthetic-password");
    await Promise.resolve();
    test.session.dispose();
    test.session.dispose();
    const updatesBeforeResponse = test.listener.mock.calls.length;
    response.resolve(listing());
    await unlocking;
    expect(test.listener).toHaveBeenCalledTimes(updatesBeforeResponse);
    expect(test.api.privacyLock).toHaveBeenCalledTimes(2);
    expect(JSON.stringify(test.history)).not.toContain("synthetic-private-name");
  });

  it("makes a new mount wait for the old cleanup lock before allowing unlock", async () => {
    const locks = new PrivacyLockQueue();
    const cleanup = deferred<void>();
    const old = setup({}, locks);
    await old.session.initialize();
    vi.mocked(old.api.privacyLock).mockReturnValueOnce(cleanup.promise);
    old.session.dispose();
    const next = setup({}, locks);
    const initialization = next.session.initialize();
    await next.session.unlock("synthetic-password");
    expect(next.api.privacyLock).not.toHaveBeenCalled();
    expect(next.api.privacyRegistered).not.toHaveBeenCalled();
    expect(next.api.privacyUnlock).not.toHaveBeenCalled();
    cleanup.resolve();
    await initialization;
    expect(next.api.privacyLock).toHaveBeenCalledTimes(1);
    expect(next.getState().phase).toBe("locked");
  });

  it("does not let another account's cleanup delay or expose the new account", async () => {
    const locks = new PrivacyLockQueue();
    const cleanup = deferred<void>();
    const old = setup({}, locks);
    await old.session.initialize();
    vi.mocked(old.api.privacyLock).mockReturnValueOnce(cleanup.promise);
    old.session.dispose();
    const next = setup({}, locks, "account-two");
    await next.session.initialize();
    await next.session.unlock("synthetic-password-two");
    expect(next.api.privacyListDir).toHaveBeenCalledWith("account-two", "2", 1, 100);
    expect(next.api.privacyLock).toHaveBeenCalledWith("account-two");
    cleanup.resolve();
  });

  it("locks again after an in-flight unlock before a new mount may unlock", async () => {
    const locks = new PrivacyLockQueue();
    const response = deferred<void>();
    const old = setup({ privacyUnlock: vi.fn().mockReturnValue(response.promise) }, locks);
    await old.session.initialize();
    const unlocking = old.session.unlock("synthetic-password");
    old.session.dispose();
    expect(old.api.privacyLock).toHaveBeenCalledTimes(2);
    const next = setup({}, locks);
    const initialization = next.session.initialize();
    expect(next.api.privacyLock).not.toHaveBeenCalled();
    await next.session.unlock("new-password");
    expect(next.api.privacyUnlock).not.toHaveBeenCalled();
    response.resolve();
    await unlocking;
    await initialization;
    expect(old.api.privacyLock).toHaveBeenCalledTimes(3);
    await next.session.unlock("new-password");
    expect(next.api.privacyUnlock).toHaveBeenCalledTimes(1);
    expect(old.api.privacyListDir).not.toHaveBeenCalled();
  });
});
