import { ArrowUpDown, Moon, Sun, Wifi } from "lucide-react";
import Sidebar from "../components/Sidebar";
import { cn } from "../lib/utils";
import { useAppStore } from "../store";
import FilesPage from "./FilesPage";
import RecycleBinPage from "./RecycleBinPage";
import TransfersPage from "./TransfersPage";
import SettingsPage from "./SettingsPage";
import PrivacyPage from "./PrivacyPage";

const titles = {
  files: "我的网盘",
  privacy: "隐私空间",
  recycle: "回收站",
  transfers: "传输中心",
  settings: "设置",
};

export default function MainShell() {
  const page = useAppStore((state) => state.page);
  const setPage = useAppStore((state) => state.setPage);
  const transfers = useAppStore((state) => state.transfers);
  const settings = useAppStore((state) => state.settings);
  const setSettings = useAppStore((state) => state.setSettings);
  const active = transfers.filter((item) =>
    ["queued", "running", "preparing", "committing"].includes(item.status),
  ).length;
  const dark =
    settings.theme === "dark" ||
    (settings.theme === "system" &&
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);

  return (
    <div className="flex h-full overflow-hidden bg-[var(--bg)]">
      <Sidebar />
      <main className="flex min-w-0 flex-1 flex-col">
        <header
          data-tauri-drag-region
          className="flex h-[58px] shrink-0 items-center justify-between border-b border-[var(--line)] bg-[var(--panel)]/72 px-6 backdrop-blur-xl"
        >
          <div className="flex items-center gap-3">
            <h1 className="text-[15px] font-semibold tracking-tight text-[var(--ink)]">
              {titles[page]}
            </h1>
            <span className="hidden items-center gap-1.5 rounded-full bg-[var(--panel-2)] px-2.5 py-1 text-[10px] text-[var(--muted)] md:inline-flex">
              <Wifi className="h-3 w-3 text-emerald-500" />
              已连接
            </span>
          </div>
          <div className="no-drag flex items-center gap-2">
            <button
              onClick={() => setPage("transfers")}
              className={cn(
                "inline-flex items-center gap-2 rounded-xl border border-[var(--line)] bg-[var(--panel)] px-3 py-2 text-[11px] text-[var(--muted)] transition hover:text-[var(--ink)]",
                active > 0 && "border-[var(--accent)]/30 text-[var(--accent)]",
              )}
            >
              <ArrowUpDown className="h-3.5 w-3.5" />
              {active > 0 ? `${active} 个任务进行中` : "传输中心"}
            </button>
            <button
              onClick={() =>
                setSettings({
                  ...settings,
                  theme: dark ? "light" : "dark",
                })
              }
              className="flex h-9 w-9 items-center justify-center rounded-xl border border-[var(--line)] bg-[var(--panel)] text-[var(--muted)] transition hover:text-[var(--ink)]"
              aria-label="切换主题"
            >
              {dark ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
            </button>
          </div>
        </header>
        <div className="min-h-0 flex-1">
          {page === "files" ? <FilesPage /> : null}
          {page === "privacy" ? <PrivacyPage /> : null}
          {page === "recycle" ? <RecycleBinPage /> : null}
          {page === "transfers" ? <TransfersPage /> : null}
          {page === "settings" ? <SettingsPage /> : null}
        </div>
      </main>
    </div>
  );
}
