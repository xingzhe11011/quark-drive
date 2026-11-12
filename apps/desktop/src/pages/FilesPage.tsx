import { useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  AlertTriangle,
  Archive,
  ChevronRight,
  CloudUpload,
  Download,
  File,
  FileAudio,
  FileCode2,
  FileImage,
  FileSpreadsheet,
  FileText,
  FileVideo,
  Folder,
  FolderPlus,
  HardDrive,
  LayoutGrid,
  List,
  Loader2,
  MoreHorizontal,
  MoveRight,
  Pencil,
  RefreshCw,
  Search,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { toast } from "sonner";
import { backend } from "../lib/backend";
import { classifyBackendError, fileListErrorState } from "../lib/errors";
import { insertFile, removeFiles, renameFile } from "../lib/file-mutations";
import type { FileItem, ListResult } from "../lib/types";
import { cn, formatBytes, formatDate } from "../lib/utils";
import { useAppStore } from "../store";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Checkbox } from "../components/ui/checkbox";
import { Dialog } from "../components/ui/dialog";
import { Dropdown, DropdownItem, DropdownSeparator } from "../components/ui/dropdown";
import { Input } from "../components/ui/input";
import { Tooltip } from "../components/ui/tooltip";
import { MoveDestinationDialog } from "../components/MoveDestinationDialog";

function extensionOf(name: string) {
  const index = name.lastIndexOf(".");
  return index > 0 ? name.slice(index + 1).toLowerCase() : "";
}

function TypeIcon({ item, size = "md" }: { item: FileItem; size?: "sm" | "md" | "lg" }) {
  const dimension = size === "lg" ? "h-10 w-10" : size === "sm" ? "h-7 w-7" : "h-9 w-9";
  const iconSize = size === "lg" ? "h-5 w-5" : size === "sm" ? "h-3.5 w-3.5" : "h-4.5 w-4.5";
  if (item.isDir) {
    return (
      <div className={cn(dimension, "flex items-center justify-center rounded-xl bg-amber-400/12 text-amber-500")}>
        <Folder className={iconSize} strokeWidth={2.1} />
      </div>
    );
  }
  const ext = extensionOf(item.name);
  const config =
    item.category === 1 || ["mp4", "mkv", "mov", "avi", "webm"].includes(ext)
      ? { icon: FileVideo, className: "bg-violet-500/12 text-violet-500" }
      : ["jpg", "jpeg", "png", "gif", "webp", "heic", "svg"].includes(ext)
        ? { icon: FileImage, className: "bg-cyan-500/12 text-cyan-500" }
        : ["mp3", "flac", "wav", "aac", "m4a"].includes(ext)
          ? { icon: FileAudio, className: "bg-pink-500/12 text-pink-500" }
          : ["zip", "rar", "7z", "tar", "gz", "xz"].includes(ext)
            ? { icon: Archive, className: "bg-orange-500/12 text-orange-500" }
            : ["js", "ts", "tsx", "rs", "go", "py", "json", "toml", "yaml", "yml", "sh"].includes(ext)
              ? { icon: FileCode2, className: "bg-emerald-500/12 text-emerald-500" }
              : ["xls", "xlsx", "csv", "numbers"].includes(ext)
                ? { icon: FileSpreadsheet, className: "bg-green-500/12 text-green-500" }
                : ["pdf", "doc", "docx", "md", "txt", "pptx"].includes(ext)
                  ? { icon: FileText, className: "bg-blue-500/12 text-blue-500" }
                  : { icon: File, className: "bg-slate-500/12 text-slate-500" };
  const Icon = config.icon;
  return (
    <div className={cn(dimension, "flex items-center justify-center rounded-xl", config.className)}>
      <Icon className={iconSize} strokeWidth={2} />
    </div>
  );
}

export default function FilesPage() {
  const account = useAppStore((state) => state.account)!;
  const crumbs = useAppStore((state) => state.crumbs);
  const enterFolder = useAppStore((state) => state.enterFolder);
  const jumpTo = useAppStore((state) => state.jumpTo);
  const viewMode = useAppStore((state) => state.viewMode);
  const setViewMode = useAppStore((state) => state.setViewMode);
  const selection = useAppStore((state) => state.selection);
  const setSelection = useAppStore((state) => state.setSelection);
  const toggleSelection = useAppStore((state) => state.toggleSelection);
  const clearSelection = useAppStore((state) => state.clearSelection);
  const search = useAppStore((state) => state.search);
  const setSearch = useAppStore((state) => state.setSearch);
  const setPage = useAppStore((state) => state.setPage);
  const queryClient = useQueryClient();
  const current = crumbs[crumbs.length - 1];
  const scrollRef = useRef<HTMLDivElement>(null);

  const [newFolderOpen, setNewFolderOpen] = useState(false);
  const [folderName, setFolderName] = useState("新建文件夹");
  const [renameTarget, setRenameTarget] = useState<FileItem | null>(null);
  const [renameName, setRenameName] = useState("");
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [moveOpen, setMoveOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  const query = useQuery({
    queryKey: ["files", account.id, current.fid],
    queryFn: () => backend.listDir(account.id, current.fid),
    retry: (failureCount, error) => classifyBackendError(error) === "other" && failureCount < 1,
  });

  const files = useMemo(() => {
    const entries = query.data?.entries ?? [];
    const keyword = search.trim().toLowerCase();
    if (!keyword) return entries;
    return entries.filter((item) => item.name.toLowerCase().includes(keyword));
  }, [query.data, search]);

  const selectedItems = useMemo(
    () => files.filter((item) => selection.includes(item.fid)),
    [files, selection],
  );
  const allSelected = files.length > 0 && selectedItems.length === files.length;

  const virtualizer = useVirtualizer({
    count: files.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 56,
    overscan: 10,
  });

  async function refresh() {
    await query.refetch();
  }

  function updateCurrentFolder(
    updater: (previous: ListResult | undefined) => ListResult | undefined,
  ) {
    queryClient.setQueryData<ListResult>(
      ["files", account.id, current.fid],
      updater,
    );
  }

  function reconcileAfterMutation(reapply?: () => void) {
    void (async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 700));
      await refresh();
      reapply?.();
    })();
  }

  async function upload(kind: "files" | "folder") {
    try {
      const paths =
        kind === "files"
          ? await backend.pickFiles()
          : await backend.pickFolder().then((path) => (path ? [path] : []));
      if (!paths.length) return;
      setBusy(true);
      const ids = await backend.enqueueUpload(account.id, paths, current.fid);
      void queryClient.invalidateQueries({ queryKey: ["files", account.id] });
      toast.success(`已加入上传队列（${ids.length} 个文件）`);
      setPage("transfers");
    } catch (error) {
      toast.error(`上传失败：${error}`);
    } finally {
      setBusy(false);
    }
  }

  async function download(items: FileItem[]) {
    if (!items.length) return;
    try {
      const ids = await backend.enqueueDownload(
        account.id,
        items.map((item) => ({
          fid: item.fid,
          name: item.name,
          size: item.size,
          isDir: item.isDir,
        })),
      );
      toast.success(`已加入下载队列（${ids.length} 项）`);
      setPage("transfers");
    } catch (error) {
      toast.error(`下载失败：${error}`);
    }
  }

  async function createFolder() {
    if (!folderName.trim()) return;
    try {
      setBusy(true);
      const name = folderName.trim();
      const fid = await backend.mkdir(account.id, current.fid, name);
      const created: FileItem | undefined = fid
        ? {
            fid,
            parentFid: current.fid,
            name,
            isDir: true,
            size: 0,
            category: 0,
            formatType: "",
            updatedAt: Date.now(),
            thumbnail: "",
          }
        : undefined;
      if (created) {
        updateCurrentFolder((previous) => insertFile(previous, created));
      }
      setNewFolderOpen(false);
      setFolderName("新建文件夹");
      toast.success("文件夹已创建");
      reconcileAfterMutation(
        created
          ? () => updateCurrentFolder((previous) => insertFile(previous, created))
          : undefined,
      );
    } catch (error) {
      toast.error(`创建失败：${error}`);
    } finally {
      setBusy(false);
    }
  }

  async function confirmRename() {
    const target = renameTarget;
    const name = renameName.trim();
    if (!target || !name) return;
    try {
      setBusy(true);
      await backend.rename(account.id, target.fid, name);
      updateCurrentFolder((previous) => renameFile(previous, target.fid, name));
      setRenameTarget(null);
      toast.success("已重命名");
      reconcileAfterMutation(() =>
        updateCurrentFolder((previous) => renameFile(previous, target.fid, name)),
      );
    } catch (error) {
      toast.error(`重命名失败：${error}`);
    } finally {
      setBusy(false);
    }
  }

  async function confirmDelete() {
    if (!selectedItems.length) return;
    const fids = selectedItems.map((item) => item.fid);
    try {
      setBusy(true);
      await backend.deleteFiles(account.id, fids);
      updateCurrentFolder((previous) => removeFiles(previous, fids));
      void queryClient.invalidateQueries({ queryKey: ["recycle", account.id] });
      clearSelection();
      setDeleteOpen(false);
      toast.success("已移入回收站");
      reconcileAfterMutation(() =>
        updateCurrentFolder((previous) => removeFiles(previous, fids)),
      );
    } catch (error) {
      toast.error(`删除失败：${error}`);
    } finally {
      setBusy(false);
    }
  }

  function openItem(item: FileItem) {
    if (item.isDir) {
      enterFolder(item.fid, item.name);
    } else {
      void download([item]);
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col px-6 pb-5 pt-5">
      <div className="mb-4 flex items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-1 overflow-x-auto">
          <HardDrive className="mr-2 h-4 w-4 shrink-0 text-[var(--muted)]" />
          {crumbs.map((crumb, index) => (
            <div key={`${crumb.fid}-${index}`} className="flex shrink-0 items-center gap-1">
              {index > 0 ? (
                <ChevronRight className="h-3.5 w-3.5 text-[var(--muted)]/50" />
              ) : null}
              <button
                onClick={() => jumpTo(index)}
                className={cn(
                  "max-w-[180px] truncate rounded-lg px-1.5 py-1 text-[12px] transition hover:bg-[var(--panel-2)]",
                  index === crumbs.length - 1
                    ? "font-medium text-[var(--ink)]"
                    : "text-[var(--muted)]",
                )}
              >
                {crumb.name}
              </button>
            </div>
          ))}
        </div>
        <div className="flex items-center gap-2">
          <Tooltip content="刷新">
            <Button variant="ghost" size="icon" onClick={refresh}>
              <RefreshCw className={cn("h-4 w-4", query.isFetching && "animate-spin")} />
            </Button>
          </Tooltip>
          <div className="flex rounded-xl border border-[var(--line)] bg-[var(--panel)] p-0.5">
            <button
              onClick={() => setViewMode("list")}
              className={cn(
                "flex h-7 w-7 items-center justify-center rounded-lg transition",
                viewMode === "list"
                  ? "bg-[var(--accent-soft)] text-[var(--accent)]"
                  : "text-[var(--muted)] hover:text-[var(--ink)]",
              )}
              aria-label="列表视图"
            >
              <List className="h-3.5 w-3.5" />
            </button>
            <button
              onClick={() => setViewMode("grid")}
              className={cn(
                "flex h-7 w-7 items-center justify-center rounded-lg transition",
                viewMode === "grid"
                  ? "bg-[var(--accent-soft)] text-[var(--accent)]"
                  : "text-[var(--muted)] hover:text-[var(--ink)]",
              )}
              aria-label="网格视图"
            >
              <LayoutGrid className="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </div>

      <div
        className={cn(
          "mb-4 flex h-9 shrink-0 items-center justify-between gap-3 rounded-xl transition-colors",
          selection.length > 0 && "border border-[var(--accent)]/20 bg-[var(--accent-soft)] px-3.5",
        )}
      >
        {selection.length > 0 ? (
          <>
            <div className="flex items-center gap-2 text-[12px] text-[var(--accent)]">
              <Badge tone="accent">{selection.length}</Badge>
              已选择
            </div>
            <div className="flex items-center gap-1.5">
              <Button size="sm" variant="ghost" onClick={() => download(selectedItems)}>
                <Download className="h-3.5 w-3.5" />
                下载
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setMoveOpen(true)}>
                <MoveRight className="h-3.5 w-3.5" />
                移动
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setDeleteOpen(true)}>
                <Trash2 className="h-3.5 w-3.5" />
                删除
              </Button>
              <Button size="sm" variant="ghost" onClick={clearSelection}>
                取消选择
              </Button>
            </div>
          </>
        ) : (
          <>
            <div className="flex items-center gap-2">
              <Dropdown
                align="start"
                trigger={
                  <Button size="md" disabled={busy}>
                    {busy ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <Upload className="h-4 w-4" />
                    )}
                    上传
                  </Button>
                }
              >
                <DropdownItem onSelect={() => upload("files")}>
                  <CloudUpload className="h-3.5 w-3.5" />
                  上传文件
                </DropdownItem>
                <DropdownItem onSelect={() => upload("folder")}>
                  <Folder className="h-3.5 w-3.5" />
                  上传文件夹
                </DropdownItem>
              </Dropdown>
              <Button variant="secondary" onClick={() => setNewFolderOpen(true)}>
                <FolderPlus className="h-4 w-4" />
                新建文件夹
              </Button>
            </div>
            <div className="relative w-[260px] max-w-full">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--muted)]" />
              <Input
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder="搜索当前目录"
                className="h-9 pl-9 pr-9 text-xs"
              />
              {search ? (
                <button
                  onClick={() => setSearch("")}
                  className="absolute right-2.5 top-1/2 -translate-y-1/2 rounded-md p-0.5 text-[var(--muted)] hover:text-[var(--ink)]"
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              ) : null}
            </div>
          </>
        )}
      </div>

      <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-2xl border border-[var(--line)] bg-[var(--panel)]">
        {query.isError ? (
          <FileListError error={query.error} retrying={query.isFetching} onRetry={refresh} />
        ) : viewMode === "list" ? (
          <>
            <div className="grid shrink-0 grid-cols-[44px_minmax(0,1fr)_120px_150px_48px] items-center border-b border-[var(--line)] px-3 py-2.5 text-[10px] font-medium uppercase tracking-wider text-[var(--muted)]">
              <div className="flex justify-center">
                <Checkbox
                  checked={allSelected}
                  onCheckedChange={() =>
                    allSelected ? clearSelection() : setSelection(files.map((item) => item.fid))
                  }
                  aria-label="全选"
                />
              </div>
              <div>名称</div>
              <div className="text-right">大小</div>
              <div className="pl-6">修改时间</div>
              <div />
            </div>
            <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto">
              {query.isLoading ? (
                <div className="space-y-2 p-3">
                  {Array.from({ length: 7 }).map((_, index) => (
                    <div key={index} className="skeleton h-12 rounded-xl" />
                  ))}
                </div>
              ) : files.length === 0 ? (
                <EmptyState
                  search={search}
                  onUpload={() => upload("files")}
                  onCreate={() => setNewFolderOpen(true)}
                />
              ) : (
                <div
                  style={{
                    height: `${virtualizer.getTotalSize()}px`,
                    position: "relative",
                  }}
                >
                  {virtualizer.getVirtualItems().map((virtualRow) => {
                    const item = files[virtualRow.index];
                    const selected = selection.includes(item.fid);
                    return (
                      <div
                        key={item.fid}
                        className={cn(
                          "file-row absolute left-0 top-0 grid w-full grid-cols-[44px_minmax(0,1fr)_120px_150px_48px] items-center border-b border-[var(--line)]/55 px-3 text-[13px] transition",
                          selected
                            ? "bg-[var(--accent-soft)]/70"
                            : "hover:bg-[var(--panel-2)]",
                        )}
                        style={{
                          height: `${virtualRow.size}px`,
                          transform: `translateY(${virtualRow.start}px)`,
                        }}
                        onDoubleClick={() => openItem(item)}
                      >
                        <div className="flex justify-center" onClick={(event) => event.stopPropagation()}>
                          <Checkbox
                            checked={selected}
                            onCheckedChange={() => toggleSelection(item.fid)}
                            aria-label={`选择 ${item.name}`}
                          />
                        </div>
                        <button
                          className="flex min-w-0 items-center gap-3 pr-4 text-left"
                          onClick={() => toggleSelection(item.fid)}
                          onDoubleClick={(event) => {
                            event.stopPropagation();
                            openItem(item);
                          }}
                        >
                          <TypeIcon item={item} />
                          <div className="min-w-0">
                            <div className="truncate font-medium text-[var(--ink)]">
                              {item.name}
                            </div>
                            <div className="mt-0.5 text-[10px] text-[var(--muted)]/70">
                              {item.isDir ? "文件夹" : (item.formatType || extensionOf(item.name) || "文件").toUpperCase()}
                            </div>
                          </div>
                        </button>
                        <div className="text-right text-[11px] tabular-nums text-[var(--muted)]">
                          {item.isDir ? "—" : formatBytes(item.size)}
                        </div>
                        <div className="pl-6 text-[11px] text-[var(--muted)]">
                          {formatDate(item.updatedAt)}
                        </div>
                        <div className="row-actions flex justify-end">
                          <Dropdown
                            trigger={
                              <button
                                className="rounded-lg p-1.5 text-[var(--muted)] transition hover:bg-[var(--line)]/60 hover:text-[var(--ink)]"
                                onClick={(event) => event.stopPropagation()}
                              >
                                <MoreHorizontal className="h-4 w-4" />
                              </button>
                            }
                          >
                            <DropdownItem onSelect={() => openItem(item)}>
                              {item.isDir ? (
                                <Folder className="h-3.5 w-3.5" />
                              ) : (
                                <Download className="h-3.5 w-3.5" />
                              )}
                              {item.isDir ? "打开" : "下载"}
                            </DropdownItem>
                            <DropdownItem
                              onSelect={() => {
                                setRenameTarget(item);
                                setRenameName(item.name);
                              }}
                            >
                              <Pencil className="h-3.5 w-3.5" />
                              重命名
                            </DropdownItem>
                            <DropdownItem
                              onSelect={() => {
                                setSelection([item.fid]);
                                setMoveOpen(true);
                              }}
                            >
                              <MoveRight className="h-3.5 w-3.5" />
                              移动到…
                            </DropdownItem>
                            <DropdownSeparator />
                            <DropdownItem
                              danger
                              onSelect={() => {
                                setSelection([item.fid]);
                                setDeleteOpen(true);
                              }}
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                              删除
                            </DropdownItem>
                          </Dropdown>
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          </>
        ) : (
          <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto p-4">
            {query.isLoading ? (
              <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4">
                {Array.from({ length: 8 }).map((_, index) => (
                  <div key={index} className="skeleton h-36 rounded-2xl" />
                ))}
              </div>
            ) : files.length === 0 ? (
              <EmptyState
                search={search}
                onUpload={() => upload("files")}
                onCreate={() => setNewFolderOpen(true)}
              />
            ) : (
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5">
                {files.map((item) => {
                  const selected = selection.includes(item.fid);
                  return (
                    <div
                      key={item.fid}
                      onDoubleClick={() => openItem(item)}
                      onClick={() => toggleSelection(item.fid)}
                      className={cn(
                        "group relative cursor-default rounded-2xl border p-4 transition",
                        selected
                          ? "border-[var(--accent)]/40 bg-[var(--accent-soft)]/60"
                          : "border-[var(--line)] bg-[var(--panel-2)]/50 hover:border-[var(--accent)]/25 hover:bg-[var(--panel-2)]",
                      )}
                    >
                      <div className="absolute left-3 top-3">
                        <Checkbox
                          checked={selected}
                          onCheckedChange={() => toggleSelection(item.fid)}
                          aria-label={`选择 ${item.name}`}
                        />
                      </div>
                      <div className="mt-5 flex justify-center">
                        <TypeIcon item={item} size="lg" />
                      </div>
                      <div className="mt-4 truncate text-center text-[12px] font-medium text-[var(--ink)]">
                        {item.name}
                      </div>
                      <div className="mt-1 text-center text-[10px] text-[var(--muted)]">
                        {item.isDir ? "文件夹" : formatBytes(item.size)}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}
      </div>

      <Dialog
        open={newFolderOpen}
        onOpenChange={setNewFolderOpen}
        title="新建文件夹"
        description="文件夹会创建在当前目录。"
        footer={
          <>
            <Button variant="ghost" onClick={() => setNewFolderOpen(false)}>
              取消
            </Button>
            <Button onClick={createFolder} disabled={busy || !folderName.trim()}>
              创建
            </Button>
          </>
        }
      >
        <Input
          value={folderName}
          onChange={(event) => setFolderName(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && createFolder()}
          autoFocus
        />
      </Dialog>

      <Dialog
        open={Boolean(renameTarget)}
        onOpenChange={(open) => !open && setRenameTarget(null)}
        title="重命名"
        footer={
          <>
            <Button variant="ghost" onClick={() => setRenameTarget(null)}>
              取消
            </Button>
            <Button onClick={confirmRename} disabled={busy || !renameName.trim()}>
              保存
            </Button>
          </>
        }
      >
        <Input
          value={renameName}
          onChange={(event) => setRenameName(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && confirmRename()}
          autoFocus
        />
      </Dialog>

      <Dialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={`删除 ${selectedItems.length} 项？`}
        description="文件会移入回收站，可随时在回收站中恢复。"
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleteOpen(false)}>
              取消
            </Button>
            <Button variant="danger" onClick={confirmDelete} disabled={busy}>
              删除
            </Button>
          </>
        }
      />

      <MoveDestinationDialog
        open={moveOpen}
        onOpenChange={setMoveOpen}
        accountId={account.id}
        fids={selectedItems.map((item) => item.fid)}
        currentParentFid={current.fid}
        onMoved={async (movedFids, destinationFid) => {
          updateCurrentFolder((previous) => removeFiles(previous, movedFids));
          clearSelection();
          void queryClient.invalidateQueries({
            queryKey: ["files", account.id, destinationFid],
          });
          await new Promise((resolve) => window.setTimeout(resolve, 700));
          await refresh();
          updateCurrentFolder((previous) => removeFiles(previous, movedFids));
        }}
      />
    </div>
  );
}

function FileListError({
  error,
  retrying,
  onRetry,
}: {
  error: unknown;
  retrying: boolean;
  onRetry: () => void;
}) {
  const state = fileListErrorState(error);
  return (
    <div role="alert" className="flex h-full min-h-[360px] flex-col items-center justify-center px-6 text-center">
      <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-amber-400/12 text-amber-500">
        <AlertTriangle className="h-7 w-7" />
      </div>
      <div className="mt-4 text-sm font-medium text-[var(--ink)]">{state.title}</div>
      <div className="mt-1.5 max-w-[340px] text-xs leading-5 text-[var(--muted)]">
        {state.description}
      </div>
      {state.canRetry ? (
        <Button className="mt-5" size="sm" variant="secondary" onClick={onRetry} disabled={retrying}>
          <RefreshCw className={cn("h-3.5 w-3.5", retrying && "animate-spin")} />
          {retrying ? "正在重试…" : "重试"}
        </Button>
      ) : null}
    </div>
  );
}

function EmptyState({
  search,
  onUpload,
  onCreate,
}: {
  search: string;
  onUpload: () => void;
  onCreate: () => void;
}) {
  return (
    <div className="flex h-full min-h-[360px] flex-col items-center justify-center px-6 text-center">
      <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-[var(--accent-soft)] text-[var(--accent)]">
        {search ? <Search className="h-7 w-7" /> : <Folder className="h-7 w-7" />}
      </div>
      <div className="mt-4 text-sm font-medium text-[var(--ink)]">
        {search ? "没有匹配的文件" : "这里还没有文件"}
      </div>
      <div className="mt-1.5 max-w-[280px] text-[11px] leading-5 text-[var(--muted)]">
        {search
          ? "换个关键词试试，搜索只作用于当前目录。"
          : "拖入文件或点击上传，把资料放进这个文件夹。"}
      </div>
      {!search ? (
        <div className="mt-5 flex gap-2">
          <Button size="sm" onClick={onUpload}>
            <Upload className="h-3.5 w-3.5" />
            上传文件
          </Button>
          <Button size="sm" variant="secondary" onClick={onCreate}>
            <FolderPlus className="h-3.5 w-3.5" />
            新建文件夹
          </Button>
        </div>
      ) : null}
    </div>
  );
}
