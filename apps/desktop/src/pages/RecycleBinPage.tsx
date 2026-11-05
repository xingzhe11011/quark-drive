import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  ChevronLeft,
  ChevronRight,
  File,
  Folder,
  Loader2,
  RefreshCw,
  RotateCcw,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { backend } from "../lib/backend";
import type { RecycleItem } from "../lib/types";
import { cn, formatBytes, formatDate } from "../lib/utils";
import { useAppStore } from "../store";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Checkbox } from "../components/ui/checkbox";
import { Dialog } from "../components/ui/dialog";

const PAGE_SIZE = 100;

type ConfirmKind = "restore" | "delete" | "clear";

export default function RecycleBinPage() {
  const account = useAppStore((state) => state.account)!;
  const queryClient = useQueryClient();
  const [page, setPage] = useState(1);
  const [selection, setSelection] = useState<string[]>([]);
  const [confirmKind, setConfirmKind] = useState<ConfirmKind | null>(null);
  const [busy, setBusy] = useState(false);

  const query = useQuery({
    queryKey: ["recycle", account.id, page],
    queryFn: () => backend.listRecycle(account.id, page, PAGE_SIZE),
  });

  const items = query.data?.entries ?? [];
  const total = query.data?.total ?? 0;
  const pages = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const allSelected = items.length > 0 && items.every((item) => selection.includes(item.recordId));
  const selectedItems = useMemo(
    () => items.filter((item) => selection.includes(item.recordId)),
    [items, selection],
  );

  function toggleSelection(recordId: string) {
    setSelection((current) =>
      current.includes(recordId)
        ? current.filter((id) => id !== recordId)
        : [...current, recordId],
    );
  }

  function clearSelection() {
    setSelection([]);
  }

  async function refreshAfterMutation() {
    setSelection([]);
    setPage(1);
    await new Promise((resolve) => setTimeout(resolve, 700));
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["recycle", account.id] }),
      queryClient.invalidateQueries({ queryKey: ["files", account.id] }),
    ]);
  }

  async function runAction(kind: ConfirmKind) {
    if (busy) return;
    const recordIds = selectedItems.map((item) => item.recordId);
    if (kind !== "clear" && recordIds.length === 0) return;
    setBusy(true);
    try {
      if (kind === "restore") {
        await backend.restoreRecycle(account.id, recordIds);
        toast.success(`已恢复 ${recordIds.length} 项`);
      } else if (kind === "delete") {
        await backend.deleteRecycle(account.id, recordIds);
        toast.success(`已彻底删除 ${recordIds.length} 项`);
      } else {
        const removed = await backend.clearRecycle(account.id);
        toast.success(removed > 0 ? `已清空 ${removed} 项` : "回收站已经是空的");
      }
      setConfirmKind(null);
      await refreshAfterMutation();
    } catch (error) {
      toast.error(`操作失败：${error}`);
    } finally {
      setBusy(false);
    }
  }

  const confirmTitle =
    confirmKind === "restore"
      ? `恢复 ${selectedItems.length} 项？`
      : confirmKind === "delete"
        ? `彻底删除 ${selectedItems.length} 项？`
        : "清空回收站？";
  const confirmDescription =
    confirmKind === "restore"
      ? "文件会恢复到原来的目录。"
      : confirmKind === "delete"
        ? "文件将被永久删除，无法再恢复。"
        : "回收站中的所有文件都会被永久删除，无法再恢复。";

  return (
    <div className="flex h-full min-h-0 flex-col px-6 pb-5 pt-5">
      <div className="mb-4 flex h-9 shrink-0 items-center justify-between gap-3 rounded-xl transition-colors">
        {selection.length > 0 ? (
          <>
            <div className="flex items-center gap-2 text-[12px] text-[var(--accent)]">
              <Badge tone="accent">{selection.length}</Badge>
              已选择
            </div>
            <div className="flex items-center gap-1.5">
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setConfirmKind("restore")}
              >
                <RotateCcw className="h-3.5 w-3.5" />
                恢复
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setConfirmKind("delete")}
              >
                <Trash2 className="h-3.5 w-3.5" />
                彻底删除
              </Button>
              <Button size="sm" variant="ghost" onClick={clearSelection}>
                取消选择
              </Button>
            </div>
          </>
        ) : (
          <>
            <div className="flex items-center gap-2 text-[12px] text-[var(--muted)]">
              <Trash2 className="h-4 w-4" />
              回收站中还有
              <span className="font-semibold tabular-nums text-[var(--ink)]">{total}</span>
              项
            </div>
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="ghost"
                onClick={() => query.refetch()}
                disabled={query.isFetching}
              >
                <RefreshCw
                  className={cn("h-3.5 w-3.5", query.isFetching && "animate-spin")}
                />
                刷新
              </Button>
              <Button
                size="sm"
                variant="danger"
                onClick={() => setConfirmKind("clear")}
                disabled={total === 0 || busy}
              >
                清空回收站
              </Button>
            </div>
          </>
        )}
      </div>

      <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-2xl border border-[var(--line)] bg-[var(--panel)]">
        <div className="grid shrink-0 grid-cols-[44px_minmax(0,1fr)_120px_170px_88px] items-center border-b border-[var(--line)] px-3 py-2.5 text-[10px] font-medium uppercase tracking-wider text-[var(--muted)]">
          <div className="flex justify-center">
            <Checkbox
              checked={allSelected}
              onCheckedChange={() =>
                allSelected
                  ? clearSelection()
                  : setSelection(items.map((item) => item.recordId))
              }
              aria-label="全选"
            />
          </div>
          <div>名称</div>
          <div className="text-right">大小</div>
          <div className="pl-4">删除时间</div>
          <div className="text-right">操作</div>
        </div>

        <div className="min-h-0 flex-1 overflow-auto">
          {query.isLoading ? (
            <div className="space-y-2 p-3">
              {Array.from({ length: 6 }).map((_, index) => (
                <div key={index} className="skeleton h-12 rounded-xl" />
              ))}
            </div>
          ) : items.length === 0 ? (
            <div className="flex h-full min-h-[360px] flex-col items-center justify-center px-6 text-center">
              <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-[var(--accent-soft)] text-[var(--accent)]">
                <Trash2 className="h-7 w-7" />
              </div>
              <div className="mt-4 text-sm font-medium text-[var(--ink)]">
                回收站是空的
              </div>
              <div className="mt-1.5 max-w-[280px] text-xs leading-5 text-[var(--muted)]">
                从网盘删除的文件会先放进这里，可随时恢复。
              </div>
            </div>
          ) : (
            items.map((item) => {
              const selected = selection.includes(item.recordId);
              return (
                <div
                  key={item.recordId}
                  onClick={() => toggleSelection(item.recordId)}
                  className={cn(
                    "grid cursor-default grid-cols-[44px_minmax(0,1fr)_120px_170px_88px] items-center border-b border-[var(--line)]/55 px-3 text-[13px] transition",
                    selected
                      ? "bg-[var(--accent-soft)]/70"
                      : "hover:bg-[var(--panel-2)]",
                  )}
                >
                  <div
                    className="flex justify-center"
                    onClick={(event) => event.stopPropagation()}
                  >
                    <Checkbox
                      checked={selected}
                      onCheckedChange={() => toggleSelection(item.recordId)}
                      aria-label={`选择 ${item.name}`}
                    />
                  </div>
                  <div className="flex min-w-0 items-center gap-3 py-2.5 pr-4">
                    <TypeIcon item={item} />
                    <div className="min-w-0">
                      <div className="truncate font-medium text-[var(--ink)]">
                        {item.name}
                      </div>
                      <div className="mt-0.5 text-[10px] text-[var(--muted)]/70">
                        {item.isDir ? "文件夹" : (item.formatType || "文件").toUpperCase()}
                      </div>
                    </div>
                  </div>
                  <div className="text-right text-[11px] tabular-nums text-[var(--muted)]">
                    {item.isDir ? "—" : formatBytes(item.size)}
                  </div>
                  <div className="pl-4 text-[11px] text-[var(--muted)]">
                    {formatDate(item.deletedAt)}
                  </div>
                  <div
                    className="flex items-center justify-end gap-1"
                    onClick={(event) => event.stopPropagation()}
                  >
                    <button
                      onClick={() => {
                        setSelection([item.recordId]);
                        setConfirmKind("restore");
                      }}
                      className="rounded-lg p-1.5 text-[var(--muted)] transition hover:bg-[var(--line)]/60 hover:text-[var(--accent)]"
                      aria-label="恢复"
                      title="恢复"
                    >
                      <RotateCcw className="h-4 w-4" />
                    </button>
                    <button
                      onClick={() => {
                        setSelection([item.recordId]);
                        setConfirmKind("delete");
                      }}
                      className="rounded-lg p-1.5 text-[var(--muted)] transition hover:bg-red-500/10 hover:text-[var(--danger)]"
                      aria-label="彻底删除"
                      title="彻底删除"
                    >
                      <Trash2 className="h-4 w-4" />
                    </button>
                  </div>
                </div>
              );
            })
          )}
        </div>

        {pages > 1 ? (
          <div className="flex shrink-0 items-center justify-between border-t border-[var(--line)] px-4 py-2.5 text-[11px] text-[var(--muted)]">
            <span>
              第 {page} / {pages} 页
            </span>
            <div className="flex items-center gap-1.5">
              <Button
                size="sm"
                variant="ghost"
                disabled={page <= 1 || query.isFetching}
                onClick={() => {
                  setPage((current) => Math.max(1, current - 1));
                  clearSelection();
                }}
              >
                <ChevronLeft className="h-3.5 w-3.5" />
                上一页
              </Button>
              <Button
                size="sm"
                variant="ghost"
                disabled={page >= pages || query.isFetching}
                onClick={() => {
                  setPage((current) => Math.min(pages, current + 1));
                  clearSelection();
                }}
              >
                下一页
                <ChevronRight className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>
        ) : null}
      </div>

      <Dialog
        open={confirmKind !== null}
        onOpenChange={(open) => !open && !busy && setConfirmKind(null)}
        title={confirmTitle}
        description={confirmDescription}
        footer={
          <>
            <Button
              variant="ghost"
              onClick={() => setConfirmKind(null)}
              disabled={busy}
            >
              取消
            </Button>
            <Button
              variant={confirmKind === "restore" ? "default" : "danger"}
              onClick={() => confirmKind && runAction(confirmKind)}
              disabled={busy}
            >
              {busy ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : confirmKind === "restore" ? (
                <RotateCcw className="h-4 w-4" />
              ) : (
                <AlertTriangle className="h-4 w-4" />
              )}
              {confirmKind === "restore"
                ? "恢复"
                : confirmKind === "delete"
                  ? "彻底删除"
                  : "清空"}
            </Button>
          </>
        }
      />
    </div>
  );
}

function TypeIcon({ item }: { item: RecycleItem }) {
  if (item.isDir) {
    return (
      <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-amber-400/12 text-amber-500">
        <Folder className="h-4.5 w-4.5" strokeWidth={2.1} />
      </div>
    );
  }
  return (
    <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-slate-500/12 text-slate-500">
      <File className="h-4.5 w-4.5" strokeWidth={2} />
    </div>
  );
}
