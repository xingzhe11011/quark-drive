import { useEffect, useState } from "react";
import {
  ArrowUpDown,
  ChevronRight,
  Cloud,
  HardDrive,
  Settings as SettingsIcon,
  Sparkles,
  Trash2,
} from "lucide-react";
import { cn, formatBytes, formatDateOnly, vipLabel } from "../lib/utils";
import { useAppStore, type Page } from "../store";
import { Progress } from "./ui/progress";

const nav: Array<{ page: Page; label: string; icon: typeof HardDrive }> = [
  { page: "files", label: "我的网盘", icon: HardDrive },
  { page: "recycle", label: "回收站", icon: Trash2 },
  { page: "transfers", label: "传输中心", icon: ArrowUpDown },
  { page: "settings", label: "设置", icon: SettingsIcon },
];

export default function Sidebar() {
  const account = useAppStore((state) => state.account);
  const page = useAppStore((state) => state.page);
  const setPage = useAppStore((state) => state.setPage);
  const transfers = useAppStore((state) => state.transfers);
  const activeCount = transfers.filter((item) =>
    ["queued", "running", "preparing", "committing"].includes(item.status),
  ).length;
  const used = account?.usedSpace ?? 0;
  const total = account?.totalSpace ?? 1;
  const ratio = total > 0 ? (used / total) * 100 : 0;
  const initial = (account?.nickname || "Q").slice(0, 1).toUpperCase();

  return (
    <aside className="flex w-[236px] shrink-0 flex-col bg-[var(--sidebar)] px-3 pb-3 pt-5 text-white">
      <div className="flex items-center gap-2.5 px-2.5">
        <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-gradient-to-br from-[#8b7cff] to-[#5f4df0] shadow-[0_10px_24px_-12px_rgba(139,124,255,0.9)]">
          <Cloud className="h-4.5 w-4.5" strokeWidth={2.4} />
        </div>
        <div className="min-w-0">
          <div className="truncate text-[14px] font-semibold tracking-wide">
            QuarkDrive
          </div>
          <div className="text-[10px] text-white/38">Linux Desktop</div>
        </div>
      </div>

      <nav className="mt-7 space-y-1">
        <div className="px-3 pb-2 text-[10px] font-medium uppercase tracking-[0.16em] text-white/28">
          工作台
        </div>
        {nav.map(({ page: item, label, icon: Icon }) => {
          const active = page === item;
          return (
            <button
              key={item}
              onClick={() => setPage(item)}
              className={cn(
                "group flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left text-[13px] transition",
                active
                  ? "bg-white/10 text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.06)]"
                  : "text-white/50 hover:bg-white/[0.055] hover:text-white/85",
              )}
            >
              <Icon
                className={cn(
                  "h-4 w-4 transition",
                  active ? "text-[#a99cff]" : "text-white/38 group-hover:text-white/70",
                )}
              />
              <span className="flex-1">{label}</span>
              {item === "transfers" && activeCount > 0 ? (
                <span className="flex h-5 min-w-5 items-center justify-center rounded-full bg-[#7c6cff] px-1.5 text-[10px] font-semibold text-white">
                  {activeCount}
                </span>
              ) : active ? (
                <ChevronRight className="h-3.5 w-3.5 text-white/30" />
              ) : null}
            </button>
          );
        })}
      </nav>

      <div className="mt-auto space-y-3">
        <div className="rounded-2xl border border-white/8 bg-white/[0.045] p-3.5">
          <div className="mb-2 flex items-center justify-between">
            <span className="text-[11px] text-white/45">存储空间</span>
            <span className="text-[10px] text-white/32">{ratio.toFixed(1)}%</span>
          </div>
          <Progress value={ratio} className="bg-white/10" />
          <div className="mt-2.5 flex items-baseline justify-between">
            <span className="text-[11px] text-white/70">{formatBytes(used)}</span>
            <span className="text-[10px] text-white/32">
              / {formatBytes(total)}
            </span>
          </div>
          {account?.isVip ? (
            <div className="mt-3 flex items-center justify-between gap-2 rounded-lg bg-gradient-to-r from-amber-300/14 to-transparent px-2 py-1.5">
              <span className="flex items-center gap-1.5 text-[10px] font-medium text-amber-200/90">
                <Sparkles className="h-3 w-3 text-amber-300" />
                {vipLabel(account.vipType)}
              </span>
              {account.vipExpireAt ? (
                <span
                  className={cn(
                    "shrink-0 text-[9px] tabular-nums",
                    account.vipExpireAt < Date.now()
                      ? "text-red-300/80"
                      : "text-amber-200/55",
                  )}
                >
                  {account.vipExpireAt < Date.now()
                    ? "已过期"
                    : `${formatDateOnly(account.vipExpireAt)} 到期`}
                </span>
              ) : null}
            </div>
          ) : null}
        </div>

        <button
          onClick={() => setPage("settings")}
          className="flex w-full items-center gap-2.5 rounded-xl px-2 py-2 text-left transition hover:bg-white/[0.055]"
        >
          <AccountAvatar src={account?.avatarUrl} initial={initial} />
          <div className="min-w-0 flex-1">
            <div className="truncate text-[12px] font-medium text-white/85">
              {account?.nickname || "未登录"}
            </div>
            <div className="truncate text-[10px] text-white/32">
              设置中心
            </div>
          </div>
          <ChevronRight className="h-3.5 w-3.5 text-white/25" />
        </button>
      </div>
    </aside>
  );
}

function AccountAvatar({ src, initial }: { src?: string; initial: string }) {
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setFailed(false);
  }, [src]);

  if (src && !failed) {
    return (
      <img
        src={src}
        alt=""
        onError={() => setFailed(true)}
        className="h-8 w-8 rounded-full object-cover ring-2 ring-white/10"
      />
    );
  }
  return (
    <div className="flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-br from-[#8b7cff] to-[#4ecdc4] text-[12px] font-semibold text-white">
      {initial}
    </div>
  );
}
