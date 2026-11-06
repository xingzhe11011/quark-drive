import { useEffect, useMemo, useState } from "react";
import {
  Activity,
  ArrowDownToLine,
  ArrowUpFromLine,
  CheckCircle2,
  Clock3,
  FolderOpen,
  Gauge,
  Pause,
  Play,
  RotateCcw,
  Trash2,
  X,
} from "lucide-react";
import { toast } from "sonner";
import { backend } from "../lib/backend";
import type { Transfer } from "../lib/types";
import {
  cn,
  formatBytes,
  formatSpeed,
  percent,
  phaseLabel,
} from "../lib/utils";
import { useAppStore } from "../store";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Progress } from "../components/ui/progress";
import { Tooltip } from "../components/ui/tooltip";

const activeStatuses = ["queued", "running", "preparing", "committing"];

export default function TransfersPage() {
  const transfers = useAppStore((state) => state.transfers);
  const setTransfers = useAppStore((state) => state.setTransfers);
  const [tab, setTab] = useState<"active" | "finished">("active");
  const [busyId, setBusyId] = useState<string | null>(null);

  useEffect(() => {
    backend
      .listTransfers()
      .then(setTransfers)
      .catch((error) => toast.error(`读取任务失败：${error}`));
  }, [setTransfers]);

  const active = useMemo(
    () => transfers.filter((item) => activeStatuses.includes(item.status)),
    [transfers],
  );
  const finished = useMemo(
    () => transfers.filter((item) => !activeStatuses.includes(item.status)),
    [transfers],
  );
  const totalSpeed = active.reduce((sum, item) => sum + item.speedBps, 0);
  const completed = transfers.filter((item) => item.status === "done").length;
  const failed = transfers.filter((item) => item.status === "failed").length;
  const visible = tab === "active" ? active : finished;

  async function act(id: string, action: () => Promise<void>) {
    setBusyId(id);
    try {
      await action();
      await backend.listTransfers().then(setTransfers);
    } catch (error) {
      toast.error(`${error}`);
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-4 px-6 pb-5 pt-5">
      <div className="grid grid-cols-2 gap-3 xl:grid-cols-4">
        <StatCard
          icon={Activity}
          label="进行中"
          value={`${active.length}`}
          hint={`${formatSpeed(totalSpeed)} 总速度`}
          tone="violet"
        />
        <StatCard
          icon={CheckCircle2}
          label="已完成"
          value={`${completed}`}
          hint="本次会话"
          tone="emerald"
        />
        <StatCard
          icon={Gauge}
          label="并发任务"
          value={`${useAppStore.getState().settings.concurrency}`}
          hint="可在设置中调整"
          tone="cyan"
        />
        <StatCard
          icon={Clock3}
          label="失败任务"
          value={`${failed}`}
          hint={failed ? "点击任务可重试" : "运行良好"}
          tone="amber"
        />
      </div>

      <div className="flex items-center justify-between">
        <div className="flex rounded-xl border border-[var(--line)] bg-[var(--panel)] p-1">
          {(
            [
              ["active", `进行中 ${active.length}`],
              ["finished", `已结束 ${finished.length}`],
            ] as const
          ).map(([key, label]) => (
            <button
              key={key}
              onClick={() => setTab(key)}
              className={cn(
                "rounded-lg px-3.5 py-1.5 text-[12px] transition",
                tab === key
                  ? "bg-[var(--accent-soft)] font-medium text-[var(--accent)]"
                  : "text-[var(--muted)] hover:text-[var(--ink)]",
              )}
            >
              {label}
            </button>
          ))}
        </div>
        <Button
          variant="ghost"
          size="sm"
          onClick={() =>
            act("clear", async () => {
              await backend.clearFinished();
              toast.success("已清理结束的任务");
            })
          }
          disabled={!finished.length}
        >
          <Trash2 className="h-3.5 w-3.5" />
          清理记录
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-auto rounded-2xl border border-[var(--line)] bg-[var(--panel)]">
        {visible.length === 0 ? (
          <div className="flex h-full min-h-[320px] flex-col items-center justify-center text-center">
            <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-[var(--accent-soft)] text-[var(--accent)]">
              <Activity className="h-7 w-7" />
            </div>
            <div className="mt-4 text-sm font-medium text-[var(--ink)]">
              {tab === "active" ? "没有正在传输的任务" : "还没有历史记录"}
            </div>
            <div className="mt-1.5 text-[11px] text-[var(--muted)]">
              上传或下载文件后，任务会出现在这里。
            </div>
          </div>
        ) : (
          <div className="divide-y divide-[var(--line)]/60">
            {visible.map((transfer) => (
              <TransferRow
                key={transfer.id}
                transfer={transfer}
                busy={busyId === transfer.id}
                onAction={(action) => act(transfer.id, action)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function StatCard({
  icon: Icon,
  label,
  value,
  hint,
  tone,
}: {
  icon: typeof Activity;
  label: string;
  value: string;
  hint: string;
  tone: "violet" | "emerald" | "cyan" | "amber";
}) {
  const tones = {
    violet: "bg-violet-500/12 text-violet-500",
    emerald: "bg-emerald-500/12 text-emerald-500",
    cyan: "bg-cyan-500/12 text-cyan-500",
    amber: "bg-amber-500/12 text-amber-500",
  };
  return (
    <div className="flex items-center gap-3 rounded-2xl border border-[var(--line)] bg-[var(--panel)] p-4">
      <div className={cn("flex h-10 w-10 shrink-0 items-center justify-center rounded-xl", tones[tone])}>
        <Icon className="h-4.5 w-4.5" />
      </div>
      <div className="min-w-0">
        <div className="text-[10px] text-[var(--muted)]">{label}</div>
        <div className="mt-0.5 text-[18px] font-semibold leading-tight tabular-nums text-[var(--ink)]">
          {value}
        </div>
        <div className="truncate text-[10px] text-[var(--muted)]/70">{hint}</div>
      </div>
    </div>
  );
}

function TransferRow({
  transfer,
  busy,
  onAction,
}: {
  transfer: Transfer;
  busy: boolean;
  onAction: (action: () => Promise<void>) => void;
}) {
  const running = activeStatuses.includes(transfer.status) && transfer.status !== "queued";
  const progress = percent(transfer.doneBytes, transfer.totalBytes);
  const Icon = transfer.kind === "upload" ? ArrowUpFromLine : ArrowDownToLine;
  const tone =
    transfer.status === "failed"
      ? "danger"
      : transfer.status === "done"
        ? "success"
        : "accent";

  return (
    <div className="flex items-center gap-4 px-4 py-4 transition hover:bg-[var(--panel-2)]/60">
      <div
        className={cn(
          "flex h-10 w-10 shrink-0 items-center justify-center rounded-xl",
          transfer.kind === "upload"
            ? "bg-violet-500/12 text-violet-500"
            : "bg-cyan-500/12 text-cyan-500",
        )}
      >
        <Icon className="h-4.5 w-4.5" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <div className="truncate text-[12.5px] font-medium text-[var(--ink)]">
            {transfer.remoteName}
          </div>
          <Badge tone={tone as "accent" | "success" | "danger"}>
            {phaseLabel(transfer.phase || transfer.status)}
          </Badge>
        </div>
        <div className="mt-1 flex items-center gap-3 text-[10px] text-[var(--muted)]">
          <span className="truncate">
            {transfer.kind === "upload" ? "上传到网盘" : "下载到本地"}
          </span>
          <span className="shrink-0 tabular-nums">
            {formatBytes(transfer.doneBytes)} / {formatBytes(transfer.totalBytes)}
          </span>
          {running ? (
            <span className="shrink-0 text-[var(--accent)]">
              {formatSpeed(transfer.speedBps)}
            </span>
          ) : null}
        </div>
        <Progress value={progress} tone={tone} className="mt-2.5" />
        {transfer.errorMessage && transfer.status === "failed" ? (
          <div className="mt-1.5 truncate text-[10px] text-[var(--danger)]">
            {transfer.errorMessage}
          </div>
        ) : null}
      </div>
      <div className="flex shrink-0 items-center gap-1">
        {running ? (
          <Tooltip content="暂停">
            <Button
              size="icon"
              variant="ghost"
              disabled={busy}
              onClick={() => onAction(() => backend.pauseTransfer(transfer.id))}
            >
              <Pause className="h-3.5 w-3.5" />
            </Button>
          </Tooltip>
        ) : null}
        {transfer.status === "paused" || transfer.status === "failed" ? (
          <Tooltip content={transfer.status === "failed" ? "重试" : "继续"}>
            <Button
              size="icon"
              variant="ghost"
              disabled={busy}
              onClick={() =>
                onAction(() =>
                  transfer.status === "failed"
                    ? backend.retryTransfer(transfer.id)
                    : backend.resumeTransfer(transfer.id),
                )
              }
            >
              {transfer.status === "failed" ? (
                <RotateCcw className="h-3.5 w-3.5" />
              ) : (
                <Play className="h-3.5 w-3.5" />
              )}
            </Button>
          </Tooltip>
        ) : null}
        {!["done", "canceled"].includes(transfer.status) ? (
          <Tooltip content="取消">
            <Button
              size="icon"
              variant="ghost"
              disabled={busy}
              onClick={() => onAction(() => backend.cancelTransfer(transfer.id))}
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          </Tooltip>
        ) : null}
        {transfer.kind === "download" && transfer.status === "done" ? (
          <Tooltip content="打开所在文件夹">
            <Button
              size="icon"
              variant="ghost"
              disabled={busy}
              onClick={() => onAction(() => backend.openPath(transfer.localPath))}
            >
              <FolderOpen className="h-3.5 w-3.5" />
            </Button>
          </Tooltip>
        ) : null}
        {["done", "canceled", "failed"].includes(transfer.status) ? (
          <Tooltip content="移除记录">
            <Button
              size="icon"
              variant="ghost"
              disabled={busy}
              onClick={() => onAction(() => backend.removeTransfer(transfer.id))}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          </Tooltip>
        ) : null}
      </div>
    </div>
  );
}
