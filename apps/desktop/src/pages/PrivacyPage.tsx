import { useEffect, useLayoutEffect, useMemo, useRef, useState, type FormEvent } from "react";
import {
  AlertCircle,
  ChevronLeft,
  ChevronRight,
  CloudUpload,
  Download,
  File,
  Folder,
  Loader2,
  LockKeyhole,
  MoveRight,
  RefreshCw,
  ShieldCheck,
  Upload,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "../components/ui/button";
import { Checkbox } from "../components/ui/checkbox";
import { Dropdown, DropdownItem } from "../components/ui/dropdown";
import { Input } from "../components/ui/input";
import { MoveDestinationDialog } from "../components/MoveDestinationDialog";
import { backend, isDesktop } from "../lib/backend";
import {
  initialPrivacyState,
  PrivacySession,
  privacyUploadDoneCount,
  PRIVACY_PAGE_SIZE,
  type PrivacyState,
} from "../lib/privacy";
import { cn, formatBytes, formatDate } from "../lib/utils";
import { useAppStore } from "../store";

function privacyGateCopy(phase: PrivacyState["phase"], desktop: boolean) {
  if (!desktop) {
    return {
      title: "浏览器预览模式",
      description: "隐私空间仅支持桌面客户端。",
    };
  }
  switch (phase) {
    case "checking":
      return { title: "正在检查隐私空间", description: "正在确认隐私空间状态。" };
    case "unlocking":
      return { title: "正在验证密码", description: "正在验证密码，请稍候。" };
    case "locking":
      return { title: "正在锁定隐私空间", description: "正在清除本地显示并锁定。" };
    case "unavailable":
      return {
        title: "尚未开通隐私空间",
        description: "请先在夸克官方客户端开通隐私空间并设置密码。",
      };
    case "failure":
      return {
        title: "隐私空间暂不可用",
        description: "请检查网络与登录状态后重试。",
      };
    default:
      return {
        title: "解锁隐私空间",
        description: "输入你在夸克中设置的隐私密码。",
      };
  }
}

export default function PrivacyPage() {
  const accountId = useAppStore((state) => state.account?.id);
  return accountId ? <PrivacyWorkspace key={accountId} accountId={accountId} /> : null;
}

function PrivacyWorkspace({ accountId }: { accountId: string }) {
  const [state, setState] = useState(initialPrivacyState);
  const [password, setPassword] = useState("");
  const [selection, setSelection] = useState<string[]>([]);
  const [moveOpen, setMoveOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const session = useRef<PrivacySession | null>(null);
  const completedUploadsByFolder = useRef(new Map<string, number>());
  const activePrivacyTransfers = useAppStore(
    (app) =>
      app.transfers.filter(
        (transfer) =>
          transfer.id.startsWith("privacy:") &&
          ["queued", "running"].includes(transfer.status),
      ).length,
  );

  const folderKey = `${state.crumbs.map((crumb) => crumb.fid).join("/")}:${state.page}:${state.phase}`;
  const currentFid = state.crumbs[state.crumbs.length - 1].fid;
  const completedUploads = useAppStore((app) =>
    privacyUploadDoneCount(app.transfers, accountId, currentFid),
  );

  useEffect(() => {
    setSelection([]);
    setMoveOpen(false);
  }, [folderKey]);

  useEffect(() => {
    if (state.phase !== "browsing") {
      completedUploadsByFolder.current.clear();
      return;
    }
    const previous = completedUploadsByFolder.current.get(folderKey);
    completedUploadsByFolder.current.set(folderKey, completedUploads);
    if (previous === undefined || completedUploads <= previous) return;
    const timer = window.setTimeout(() => {
      void session.current?.load(state.crumbs, state.page);
    }, 600);
    return () => window.clearTimeout(timer);
  }, [completedUploads, folderKey, state.crumbs, state.page, state.phase]);

  useLayoutEffect(() => {
    const current = new PrivacySession(accountId, backend);
    session.current = current;
    const unsubscribe = current.subscribe(setState);
    void current.initialize();
    return () => {
      unsubscribe();
      current.dispose();
      session.current = null;
    };
  }, [accountId]);

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const submitted = password;
    setPassword("");
    event.currentTarget.reset();
    void session.current?.unlock(submitted);
  }

  function lock() {
    setPassword("");
    setSelection([]);
    setMoveOpen(false);
    void session.current?.lock();
  }

  const sessionBusy = ["checking", "unlocking", "locking"].includes(state.phase);
  const gateWarning = state.phase === "failure" || state.phase === "unavailable";
  const showPasswordForm =
    isDesktop && (state.phase === "locked" || state.phase === "unlocking");
  const { title: gateTitle, description: gateDescription } = privacyGateCopy(
    state.phase,
    isDesktop,
  );
  const pages = Math.max(state.page, Math.ceil(state.total / PRIVACY_PAGE_SIZE), 1);
  const selectedItems = useMemo(
    () => state.entries.filter((entry) => selection.includes(entry.fid)),
    [selection, state.entries],
  );
  const allSelected =
    state.entries.length > 0 && selectedItems.length === state.entries.length;

  function toggleSelection(fid: string) {
    setSelection((current) =>
      current.includes(fid)
        ? current.filter((value) => value !== fid)
        : [...current, fid],
    );
  }

  async function upload(kind: "files" | "folder") {
    try {
      const paths =
        kind === "files"
          ? await backend.pickFiles()
          : await backend.pickFolder().then((path) => (path ? [path] : []));
      if (!paths.length) return;
      setBusy(true);
      const created = await backend.privacyEnqueueUpload(
        accountId,
        paths,
        currentFid,
      );
      toast.success(
        `已加入隐私上传任务（${created.length} 个文件），离开隐私空间会取消任务`,
      );
      await session.current?.load(state.crumbs, state.page);
    } catch {
      toast.error("隐私上传任务创建失败，请确认仍处于解锁状态");
    } finally {
      setBusy(false);
    }
  }

  async function download(items: typeof state.entries) {
    if (!items.length) return;
    try {
      setBusy(true);
      const created = await backend.privacyEnqueueDownload(
        accountId,
        items.map((item) => ({
          fid: item.fid,
          name: item.name,
          size: item.size,
          isDir: item.isDir,
        })),
      );
      setSelection([]);
      toast.success(
        `已加入隐私下载任务（${created.length} 个文件），离开隐私空间会取消任务`,
      );
    } catch {
      toast.error("隐私下载任务创建失败，请确认仍处于解锁状态");
    } finally {
      setBusy(false);
    }
  }

  function openFolder(entry: (typeof state.entries)[number]) {
    if (!entry.isDir) {
      void download([entry]);
      return;
    }
    setSelection([]);
    void session.current?.load(
      [...state.crumbs, { fid: entry.fid, name: entry.name }],
      1,
    );
  }

  return (
    <section className="flex h-full min-h-0 flex-col px-6 pb-5 pt-5" aria-label="隐私空间">
      {state.phase === "browsing" ? (
        <>
          <div className="mb-3 flex shrink-0 items-center gap-2 rounded-xl border border-[var(--accent)]/15 bg-[var(--accent-soft)] px-3.5 py-2.5 text-[11px] leading-5 text-[var(--muted)]">
            <ShieldCheck className="h-3.5 w-3.5 shrink-0 text-[var(--accent)]" />
            <span>隐私上传和下载不会写入传输记录；离开本页会自动取消任务并锁定。</span>
          </div>

          <div className="mb-3 flex h-9 shrink-0 items-center justify-between gap-3">
            <nav aria-label="隐私目录路径" className="flex min-w-0 items-center gap-1 overflow-x-auto text-[12px]">
              {state.crumbs.map((crumb, index) => (
                <div key={`${crumb.fid}-${index}`} className="flex shrink-0 items-center gap-1">
                  {index > 0 ? <ChevronRight className="h-3.5 w-3.5 text-[var(--muted)]" /> : null}
                  <button
                    className="max-w-48 truncate rounded-lg px-2 py-1.5 text-[var(--ink)] hover:bg-[var(--panel-2)] disabled:text-[var(--muted)]"
                    disabled={state.loading || index === state.crumbs.length - 1}
                    onClick={() => void session.current?.load(state.crumbs.slice(0, index + 1), 1)}
                  >
                    {crumb.name}
                  </button>
                </div>
              ))}
            </nav>
            <div className="flex shrink-0 items-center gap-2">
              <Button size="sm" variant="ghost" disabled={state.loading} onClick={() => void session.current?.load(state.crumbs, state.page)}>
                <RefreshCw className="h-3.5 w-3.5" />
                刷新
              </Button>
              <Button size="sm" variant="soft" onClick={lock}>
                <LockKeyhole className="h-3.5 w-3.5" />
                立即锁定
              </Button>
            </div>
          </div>

          <div
            className={cn(
              "mb-3 flex h-9 shrink-0 items-center justify-between gap-3 rounded-xl transition-colors",
              selection.length > 0 &&
                "border border-[var(--accent)]/20 bg-[var(--accent-soft)] px-3.5",
            )}
          >
            {selection.length > 0 ? (
              <>
                <div className="flex items-center gap-2 text-[12px] text-[var(--accent)]">
                  <span className="rounded-md bg-[var(--accent)] px-1.5 py-0.5 text-[10px] font-medium text-white">
                    {selection.length}
                  </span>
                  已选择
                </div>
                <div className="flex items-center gap-1.5">
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy}
                    onClick={() => void download(selectedItems)}
                  >
                    <Download className="h-3.5 w-3.5" />
                    下载
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy}
                    onClick={() => setMoveOpen(true)}
                  >
                    <MoveRight className="h-3.5 w-3.5" />
                    移动
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => setSelection([])}
                  >
                    取消选择
                  </Button>
                </div>
              </>
            ) : (
              <>
                <Dropdown
                  align="start"
                  trigger={
                    <Button size="md" disabled={busy || state.loading}>
                      {busy ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <Upload className="h-4 w-4" />
                      )}
                      上传
                    </Button>
                  }
                >
                  <DropdownItem onSelect={() => void upload("files")}>
                    <CloudUpload className="h-3.5 w-3.5" />
                    上传文件
                  </DropdownItem>
                  <DropdownItem onSelect={() => void upload("folder")}>
                    <Folder className="h-3.5 w-3.5" />
                    上传文件夹
                  </DropdownItem>
                </Dropdown>
                <div className="text-[11px] text-[var(--muted)]">
                  {activePrivacyTransfers > 0
                    ? `${activePrivacyTransfers} 个隐私任务进行中，请保持解锁`
                    : "选择文件后可下载或移动"}
                </div>
              </>
            )}
          </div>

          <div className="min-h-0 flex-1 overflow-auto rounded-2xl border border-[var(--line)] bg-[var(--panel)]" aria-busy={state.loading}>
            <table className="w-full table-fixed text-left text-[12px]">
              <thead className="sticky top-0 z-10 bg-[var(--panel-2)] text-[11px] text-[var(--muted)]">
                <tr>
                  <th scope="col" className="w-12 px-4 py-3">
                    <Checkbox
                      checked={allSelected}
                      onCheckedChange={() =>
                        setSelection(
                          allSelected ? [] : state.entries.map((entry) => entry.fid),
                        )
                      }
                      aria-label="全选隐私文件"
                    />
                  </th>
                  <th scope="col" className="px-3 py-3 font-medium">名称</th>
                  <th scope="col" className="w-24 px-3 py-3 font-medium">大小</th>
                  <th scope="col" className="w-36 px-4 py-3 font-medium">修改时间</th>
                </tr>
              </thead>
              <tbody>
                {state.entries.map((item) => (
                  <tr
                    key={item.fid}
                    className={cn(
                      "border-t border-[var(--line)] text-[var(--ink)] transition",
                      selection.includes(item.fid)
                        ? "bg-[var(--accent-soft)]/70"
                        : "hover:bg-[var(--panel-2)]/70",
                    )}
                    onClick={() => toggleSelection(item.fid)}
                    onDoubleClick={() => {
                      openFolder(item);
                    }}
                  >
                    <td
                      className="px-4 py-3"
                      onClick={(event) => event.stopPropagation()}
                    >
                      <Checkbox
                        checked={selection.includes(item.fid)}
                        onCheckedChange={() => toggleSelection(item.fid)}
                        aria-label={`选择 ${item.name}`}
                      />
                    </td>
                    <td className="px-3 py-3">
                      <div className="flex items-center gap-3">
                        {item.isDir ? <Folder className="h-5 w-5 shrink-0 text-amber-500" /> : <File className="h-5 w-5 shrink-0 text-[var(--muted)]" />}
                        {item.isDir ? (
                          <button
                            className="truncate text-left hover:text-[var(--accent)] focus-visible:outline-[var(--accent)]"
                            onClick={(event) => {
                              event.stopPropagation();
                              if (event.detail === 0) openFolder(item);
                            }}
                            onDoubleClick={(event) => {
                              event.stopPropagation();
                              openFolder(item);
                            }}
                            aria-label={`打开文件夹 ${item.name}`}
                          >
                            {item.name}
                          </button>
                        ) : <span className="truncate">{item.name}</span>}
                      </div>
                    </td>
                    <td className="px-3 py-3 tabular-nums text-[var(--muted)]">{item.isDir ? "—" : formatBytes(item.size)}</td>
                    <td className="px-4 py-3 text-[var(--muted)]">{formatDate(item.updatedAt)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {state.loading ? (
              <div role="status" className="flex items-center justify-center gap-2 py-20 text-[13px] text-[var(--muted)]">
                <Loader2 className="h-4 w-4 animate-spin" />
                正在读取隐私目录…
              </div>
            ) : state.entries.length === 0 ? (
              <div className="py-20 text-center text-[13px] text-[var(--muted)]">当前目录没有文件</div>
            ) : null}
          </div>

          <div className="mt-3 flex h-8 shrink-0 items-center justify-between text-[11px] text-[var(--muted)]">
            <span>
              {state.loading
                ? "正在读取…"
                : `共 ${state.total} 项 · 双击打开文件夹或下载文件`}
              {activePrivacyTransfers > 0
                ? ` · ${activePrivacyTransfers} 个隐私任务运行中`
                : ""}
            </span>
            <div className="flex items-center gap-3">
              <Button size="sm" variant="ghost" aria-label="上一页" disabled={state.loading || state.page <= 1} onClick={() => void session.current?.load(state.crumbs, state.page - 1)}>
                <ChevronLeft className="h-3.5 w-3.5" />
              </Button>
              <span className="tabular-nums">{state.page} / {pages}</span>
              <Button size="sm" variant="ghost" aria-label="下一页" disabled={state.loading || state.page >= pages} onClick={() => void session.current?.load(state.crumbs, state.page + 1)}>
                <ChevronRight className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>
        </>
      ) : (
        <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto py-8">
          <div className="relative w-full max-w-sm overflow-hidden rounded-3xl border border-[var(--line)] bg-[var(--panel)] p-7 shadow-[0_24px_70px_-38px_rgba(0,0,0,0.38)]">
            <div className="pointer-events-none absolute -right-16 -top-16 h-44 w-44 rounded-full bg-[var(--accent)]/10 blur-3xl" />
            <div className="relative flex flex-col items-center text-center">
              <div
                className={cn(
                  "flex h-16 w-16 items-center justify-center rounded-2xl",
                  gateWarning || !isDesktop
                    ? "bg-amber-500/10 text-amber-500"
                    : "bg-[var(--accent-soft)] text-[var(--accent)]",
                )}
              >
                {sessionBusy ? (
                  <Loader2 className="h-6 w-6 animate-spin" />
                ) : gateWarning || !isDesktop ? (
                  <AlertCircle className="h-6 w-6" />
                ) : (
                  <LockKeyhole className="h-6 w-6" />
                )}
              </div>
              <h2 className="mt-5 text-lg font-semibold tracking-tight text-[var(--ink)]">
                {gateTitle}
              </h2>
              <p className="mt-1.5 max-w-[280px] text-xs leading-5 text-[var(--muted)]">
                {gateDescription}
              </p>
            </div>
            {state.message ? (
              <div role="alert" className="relative mt-5 flex gap-2 rounded-xl bg-amber-500/10 px-3 py-2.5 text-[11px] leading-5 text-amber-700 dark:text-amber-300">
                <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
                <span>{state.message}</span>
              </div>
            ) : null}
            {showPasswordForm ? (
              <form className="relative mt-6 space-y-2.5" onSubmit={submit} autoComplete="off">
                <label htmlFor="privacy-password" className="sr-only">隐私密码</label>
                <Input
                  id="privacy-password"
                  name="privacy-password"
                  type="password"
                  autoComplete="off"
                  spellCheck={false}
                  autoCapitalize="none"
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                  disabled={sessionBusy}
                  placeholder="输入隐私密码"
                  className="h-11 px-3.5"
                  required
                />
                <Button type="submit" size="lg" className="w-full" disabled={sessionBusy || password.length === 0 || !isDesktop}>
                  {sessionBusy ? <Loader2 className="h-4 w-4 animate-spin" /> : <ShieldCheck className="h-4 w-4" />}
                  {sessionBusy ? "正在解锁…" : "解锁"}
                </Button>
              </form>
            ) : gateWarning && isDesktop ? (
              <Button className="relative mt-6 w-full" variant="secondary" onClick={() => void session.current?.initialize()}>
                <RefreshCw className="h-4 w-4" />
                重新检查
              </Button>
            ) : null}
            {state.phase === "unlocking" ? (
              <Button className="relative mt-2 w-full" variant="ghost" onClick={lock}>取消并锁定</Button>
            ) : null}
            {showPasswordForm ? (
              <div className="relative mt-5 space-y-2.5 rounded-2xl border border-[var(--line)]/60 bg-[var(--panel-2)]/55 px-4 py-3.5">
                <div className="flex items-start gap-2.5 text-[11px] leading-5 text-[var(--muted)]">
                  <ShieldCheck className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--accent)]" />
                  <span>密码只用于本次验证，不会保存。</span>
                </div>
                <div className="flex items-start gap-2.5 text-[11px] leading-5 text-[var(--muted)]">
                  <LockKeyhole className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--accent)]" />
                  <span>隐私上传和下载不会写入传输记录。</span>
                </div>
              </div>
            ) : null}
          </div>
        </div>
      )}

      <MoveDestinationDialog
        open={moveOpen}
        onOpenChange={setMoveOpen}
        accountId={accountId}
        fids={selectedItems.map((item) => item.fid)}
        currentParentFid={
          state.crumbs[state.crumbs.length - 1]?.fid ?? "2"
        }
        privacy
        onMoved={async (movedFids) => {
          setSelection([]);
          session.current?.forget(movedFids);
          await new Promise((resolve) => window.setTimeout(resolve, 400));
          await session.current?.load(state.crumbs, state.page);
          session.current?.forget(movedFids);
        }}
      />
    </section>
  );
}
