import { useEffect, useRef, useState } from "react";
import {
  AlertCircle,
  ChevronRight,
  Folder,
  Loader2,
  MoveRight,
} from "lucide-react";
import { toast } from "sonner";
import { backend } from "../lib/backend";
import type { FileItem } from "../lib/types";
import { cn } from "../lib/utils";
import { Button } from "./ui/button";
import { Dialog } from "./ui/dialog";

interface Crumb {
  fid: string;
  name: string;
}

export function MoveDestinationDialog({
  open,
  onOpenChange,
  accountId,
  fids,
  currentParentFid,
  privacy = false,
  onMoved,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  accountId: string;
  fids: string[];
  currentParentFid: string;
  privacy?: boolean;
  onMoved: (movedFids: string[], destinationFid: string) => void | Promise<void>;
}) {
  const root = privacy
    ? { fid: "2", name: "隐私空间" }
    : { fid: "0", name: "我的网盘" };
  const [crumbs, setCrumbs] = useState<Crumb[]>([root]);
  const [folders, setFolders] = useState<FileItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [moving, setMoving] = useState(false);
  const [error, setError] = useState("");
  const generation = useRef(0);

  useEffect(() => {
    if (!open) return;
    const current = ++generation.current;
    setCrumbs([root]);
    setFolders([]);
    setError("");
    setLoading(true);
    const request = privacy
      ? backend.privacyListDir(accountId, root.fid, 1, 200)
      : backend.listDir(accountId, root.fid, 1, 200);
    void request
      .then((result) => {
        if (current !== generation.current) return;
        setFolders(result.entries.filter((entry) => entry.isDir));
      })
      .catch(() => {
        if (current !== generation.current) return;
        setError("无法读取目录，请重试或重新解锁隐私空间。");
      })
      .finally(() => {
        if (current === generation.current) setLoading(false);
      });
  }, [accountId, open, privacy, root.fid, root.name]);

  const destination = crumbs[crumbs.length - 1];
  const blockedBySource = crumbs.some((crumb) => fids.includes(crumb.fid));
  const sameFolder = destination.fid === currentParentFid;
  const canMove = !loading && !moving && !error && !blockedBySource && !sameFolder;

  async function openFolder(folder: FileItem) {
    const next = [...crumbs, { fid: folder.fid, name: folder.name }];
    setCrumbs(next);
    setFolders([]);
    setError("");
    const current = ++generation.current;
    setLoading(true);
    try {
      const result = privacy
        ? await backend.privacyListDir(accountId, folder.fid, 1, 200)
        : await backend.listDir(accountId, folder.fid, 1, 200);
      if (current !== generation.current) return;
      setFolders(result.entries.filter((entry) => entry.isDir));
    } catch {
      if (current !== generation.current) return;
      setError("无法读取目录，请重试或重新解锁隐私空间。");
    } finally {
      if (current === generation.current) setLoading(false);
    }
  }

  function jumpTo(index: number) {
    const next = crumbs.slice(0, index + 1);
    setCrumbs(next);
    setFolders([]);
    setError("");
    const folder = next[next.length - 1];
    const current = ++generation.current;
    setLoading(true);
    const request = privacy
      ? backend.privacyListDir(accountId, folder.fid, 1, 200)
      : backend.listDir(accountId, folder.fid, 1, 200);
    void request
      .then((result) => {
        if (current !== generation.current) return;
        setFolders(result.entries.filter((entry) => entry.isDir));
      })
      .catch(() => {
        if (current !== generation.current) return;
        setError("无法读取目录，请重试或重新解锁隐私空间。");
      })
      .finally(() => {
        if (current === generation.current) setLoading(false);
      });
  }

  async function move() {
    if (!canMove) return;
    setMoving(true);
    try {
      if (privacy) {
        await backend.privacyMoveFiles(accountId, fids, destination.fid);
      } else {
        await backend.moveFiles(accountId, fids, destination.fid);
      }
      toast.success(`已移动 ${fids.length} 项到「${destination.name}」`);
      onOpenChange(false);
      await onMoved(fids, destination.fid);
    } catch {
      toast.error("移动失败，请检查目标目录后重试");
    } finally {
      setMoving(false);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!moving) onOpenChange(next);
      }}
      title={privacy ? "移动隐私文件" : "移动文件"}
      description={`选择 ${fids.length} 个项目的目标文件夹。`}
      className="w-[min(92vw,560px)]"
      footer={
        <>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={moving}>
            取消
          </Button>
          <Button onClick={move} disabled={!canMove}>
            {moving ? <Loader2 className="h-4 w-4 animate-spin" /> : <MoveRight className="h-4 w-4" />}
            {moving ? "正在移动…" : "移动到这里"}
          </Button>
        </>
      }
    >
      <div className="flex h-9 items-center gap-1 overflow-x-auto rounded-xl border border-[var(--line)] bg-[var(--panel-2)]/60 px-2">
        {crumbs.map((crumb, index) => (
          <div key={`${crumb.fid}-${index}`} className="flex shrink-0 items-center gap-1">
            {index > 0 ? <ChevronRight className="h-3 w-3 text-[var(--muted)]" /> : null}
            <button
              className="max-w-36 truncate rounded-md px-1.5 py-1 text-[11px] text-[var(--ink)] hover:bg-[var(--panel)] disabled:text-[var(--muted)]"
              disabled={moving || index === crumbs.length - 1}
              onClick={() => jumpTo(index)}
            >
              {crumb.name}
            </button>
          </div>
        ))}
      </div>

      <div className="mt-3 h-[300px] overflow-auto rounded-xl border border-[var(--line)] bg-[var(--panel)]">
        {loading ? (
          <div role="status" className="flex h-full items-center justify-center gap-2 text-[12px] text-[var(--muted)]">
            <Loader2 className="h-4 w-4 animate-spin" />
            正在读取目录…
          </div>
        ) : error ? (
          <div role="alert" className="flex h-full flex-col items-center justify-center gap-2 px-6 text-center text-[12px] text-[var(--muted)]">
            <AlertCircle className="h-5 w-5 text-amber-500" />
            {error}
          </div>
        ) : folders.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center text-center text-[12px] text-[var(--muted)]">
            <Folder className="mb-2 h-6 w-6 text-amber-500/70" />
            此目录没有子文件夹
          </div>
        ) : (
          <div className="divide-y divide-[var(--line)]/60">
            {folders.map((folder) => (
              <button
                key={folder.fid}
                className="flex w-full items-center gap-3 px-4 py-3 text-left text-[12px] text-[var(--ink)] transition hover:bg-[var(--panel-2)]"
                onDoubleClick={() => void openFolder(folder)}
                onClick={() => void openFolder(folder)}
                aria-label={`打开文件夹 ${folder.name}`}
              >
                <Folder className="h-4 w-4 shrink-0 text-amber-500" />
                <span className="min-w-0 flex-1 truncate">{folder.name}</span>
                <ChevronRight className={cn("h-3.5 w-3.5 text-[var(--muted)]")} />
              </button>
            ))}
          </div>
        )}
      </div>
      {blockedBySource ? (
        <p className="mt-2 text-[11px] text-amber-600 dark:text-amber-400">
          不能把文件夹移动到自身或其子目录。
        </p>
      ) : sameFolder ? (
        <p className="mt-2 text-[11px] text-[var(--muted)]">当前目录就是目标目录。</p>
      ) : (
        <p className="mt-2 truncate text-[11px] text-[var(--muted)]">目标：{destination.name}</p>
      )}
    </Dialog>
  );
}
