import { useState } from "react";
import {
  ArchiveRestore,
  Check,
  Cloud,
  FolderOpen,
  Gauge,
  HardDrive,
  Info,
  Loader2,
  LogOut,
  Moon,
  RefreshCw,
  Server,
  Sun,
  Monitor,
} from "lucide-react";
import { toast } from "sonner";
import { backend } from "../lib/backend";
import type { Settings } from "../lib/types";
import { cn, formatBytes, formatDateOnly, vipLabel } from "../lib/utils";
import { useAppStore } from "../store";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Dialog } from "../components/ui/dialog";
import { Input } from "../components/ui/input";
import { Progress } from "../components/ui/progress";

export default function SettingsPage() {
  const account = useAppStore((state) => state.account)!;
  const settings = useAppStore((state) => state.settings);
  const dataDir = useAppStore((state) => state.dataDir);
  const setSettings = useAppStore((state) => state.setSettings);
  const setAccount = useAppStore((state) => state.setAccount);
  const resetCrumbs = useAppStore((state) => state.resetCrumbs);

  const [downloadDir, setDownloadDir] = useState(settings.downloadDir);
  const [concurrency, setConcurrency] = useState(settings.concurrency);
  const [speedLimit, setSpeedLimit] = useState(
    settings.maxSpeedBps ? (settings.maxSpeedBps / 1024 ** 2).toFixed(1) : "0",
  );
  const [theme, setTheme] = useState(settings.theme);
  const [saving, setSaving] = useState(false);
  const [loggingOut, setLoggingOut] = useState(false);
  const [logoutOpen, setLogoutOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  const usedRatio =
    account.totalSpace > 0 ? (account.usedSpace / account.totalSpace) * 100 : 0;

  async function chooseFolder() {
    const path = await backend.pickFolder();
    if (path) setDownloadDir(path);
  }

  async function save() {
    setSaving(true);
    try {
      const next = await backend.updateSettings({
        downloadDir,
        concurrency,
        maxSpeedBps: Math.max(0, Number(speedLimit) || 0) * 1024 ** 2,
        theme,
      } satisfies Partial<Settings>);
      setSettings(next);
      toast.success("设置已保存");
    } catch (error) {
      toast.error(`保存失败：${error}`);
    } finally {
      setSaving(false);
    }
  }

  async function refresh() {
    setRefreshing(true);
    try {
      const next = await backend.refreshAccount(account.id);
      setAccount(next);
      setSettings(useAppStore.getState().settings);
      toast.success("账号信息已刷新");
    } catch (error) {
      toast.error(`刷新失败：${error}`);
    } finally {
      setRefreshing(false);
    }
  }

  async function logout() {
    setLoggingOut(true);
    try {
      await backend.logout(account.id);
      setAccount(null);
      resetCrumbs();
      toast.success("已退出登录");
    } catch (error) {
      toast.error(`退出失败：${error}`);
    } finally {
      setLoggingOut(false);
      setLogoutOpen(false);
    }
  }

  return (
    <div className="h-full overflow-auto px-6 pb-8 pt-5">
      <div className="mx-auto max-w-[900px] space-y-4">
        <section className="overflow-hidden rounded-2xl border border-[var(--line)] bg-[var(--panel)]">
          <div className="flex items-center gap-4 p-5">
            {account.avatarUrl ? (
              <img
                src={account.avatarUrl}
                alt=""
                className="h-14 w-14 rounded-2xl object-cover ring-2 ring-[var(--line)]"
              />
            ) : (
              <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-gradient-to-br from-[var(--accent)] to-cyan-400 text-xl font-semibold text-white shadow-lg">
                {(account.nickname || "Q").slice(0, 1)}
              </div>
            )}
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <div className="truncate text-[16px] font-semibold text-[var(--ink)]">
                  {account.nickname}
                </div>
                {account.isVip ? (
                  <Badge tone="warning">{vipLabel(account.vipType)}</Badge>
                ) : (
                  <Badge>普通用户</Badge>
                )}
              </div>
              <div className="mt-1 text-[11px] text-[var(--muted)]">
                UID {account.uid || "—"} · 夸克网盘账号
                {account.vipExpireAt ? (
                  <>
                    {" · "}
                    <span
                      className={cn(
                        account.vipExpireAt < Date.now() && "text-red-500",
                      )}
                    >
                      {account.vipExpireAt < Date.now()
                        ? "会员已过期"
                        : `有效期至 ${formatDateOnly(account.vipExpireAt)}`}
                    </span>
                  </>
                ) : null}
              </div>
            </div>
            <Button variant="secondary" size="sm" onClick={refresh} disabled={refreshing}>
              <RefreshCw className={cn("h-3.5 w-3.5", refreshing && "animate-spin")} />
              刷新信息
            </Button>
          </div>
          <div className="border-t border-[var(--line)] px-5 py-4">
            <div className="mb-2 flex items-center justify-between text-[11px]">
              <span className="text-[var(--muted)]">存储空间</span>
              <span className="tabular-nums text-[var(--muted)]">
                {formatBytes(account.usedSpace)} / {formatBytes(account.totalSpace)}
              </span>
            </div>
            <Progress value={usedRatio} />
          </div>
        </section>

        <section className="rounded-2xl border border-[var(--line)] bg-[var(--panel)] p-5">
          <SectionTitle icon={HardDrive} title="下载与传输" />
          <div className="mt-5 space-y-5">
            <div>
              <label className="mb-2 block text-[12px] font-medium text-[var(--ink)]">
                默认下载目录
              </label>
              <div className="flex gap-2">
                <Input
                  value={downloadDir}
                  onChange={(event) => setDownloadDir(event.target.value)}
                  className="h-9 font-mono text-[11px]"
                />
                <Button variant="secondary" size="sm" className="h-9 shrink-0" onClick={chooseFolder}>
                  <FolderOpen className="h-3.5 w-3.5" />
                  浏览
                </Button>
              </div>
            </div>

            <div className="grid gap-5 sm:grid-cols-2">
              <div>
                <label className="mb-2 flex items-center gap-1.5 text-[12px] font-medium text-[var(--ink)]">
                  <Gauge className="h-3.5 w-3.5 text-[var(--muted)]" />
                  同时传输任务数
                </label>
                <div className="flex gap-1.5">
                  {[1, 2, 3, 4, 5, 6].map((value) => (
                    <button
                      key={value}
                      onClick={() => setConcurrency(value)}
                      className={cn(
                        "h-9 flex-1 rounded-lg border text-[12px] transition",
                        concurrency === value
                          ? "border-[var(--accent)]/40 bg-[var(--accent-soft)] font-medium text-[var(--accent)]"
                          : "border-[var(--line)] text-[var(--muted)] hover:text-[var(--ink)]",
                      )}
                    >
                      {value}
                    </button>
                  ))}
                </div>
                <p className="mt-1.5 text-[10px] text-[var(--muted)]/70">
                  建议保持 2–3，过高可能触发风控。
                </p>
              </div>
              <div>
                <label className="mb-2 flex items-center gap-1.5 text-[12px] font-medium text-[var(--ink)]">
                  <Server className="h-3.5 w-3.5 text-[var(--muted)]" />
                  全局限速
                </label>
                <div className="relative">
                  <Input
                    value={speedLimit}
                    onChange={(event) => setSpeedLimit(event.target.value)}
                    inputMode="decimal"
                    className="h-9 pr-14 text-[12px]"
                  />
                  <span className="absolute right-3.5 top-1/2 -translate-y-1/2 text-[10px] text-[var(--muted)]">
                    MB/s
                  </span>
                </div>
                <p className="mt-1.5 text-[10px] text-[var(--muted)]/70">
                  填 0 表示不限制。
                </p>
              </div>
            </div>
          </div>
        </section>

        <section className="rounded-2xl border border-[var(--line)] bg-[var(--panel)] p-5">
          <SectionTitle icon={Sun} title="外观" />
          <div className="mt-4 grid grid-cols-3 gap-2">
            {[
              { value: "light", label: "浅色", icon: Sun },
              { value: "dark", label: "深色", icon: Moon },
              { value: "system", label: "跟随系统", icon: Monitor },
            ].map(({ value, label, icon: Icon }) => (
              <button
                key={value}
                onClick={() => setTheme(value)}
                className={cn(
                  "flex items-center gap-2.5 rounded-xl border px-3.5 py-3 text-left transition",
                  theme === value
                    ? "border-[var(--accent)]/40 bg-[var(--accent-soft)] text-[var(--accent)]"
                    : "border-[var(--line)] text-[var(--muted)] hover:text-[var(--ink)]",
                )}
              >
                <Icon className="h-4 w-4" />
                <span className="text-[12px] font-medium">{label}</span>
                {theme === value ? <Check className="ml-auto h-3.5 w-3.5" /> : null}
              </button>
            ))}
          </div>
        </section>

        <section className="rounded-2xl border border-[var(--line)] bg-[var(--panel)] p-5">
          <SectionTitle icon={Info} title="关于" />
          <div className="mt-4 grid gap-3 text-[11px] sm:grid-cols-2">
            <InfoRow label="版本" value="0.2.0" />
            <InfoRow label="协议" value="Quark PC API (Cookie)" />
            <InfoRow label="数据目录" value={dataDir || "—"} mono />
            <InfoRow label="凭据存储" value="本地文件 · 0600 权限" />
          </div>
          <div className="mt-4 flex items-start gap-2 rounded-xl bg-[var(--panel-2)] p-3 text-[10px] leading-5 text-[var(--muted)]">
            <Cloud className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--accent)]" />
            本项目是第三方客户端，接口由夸克网页端提供，可能随官方更新而变化。请勿用于高频批量操作。
          </div>
        </section>

        <section className="flex items-center justify-between rounded-2xl border border-red-500/20 bg-red-500/[0.035] p-5">
          <div>
            <div className="text-[12px] font-medium text-[var(--ink)]">退出登录</div>
            <div className="mt-1 text-[10px] text-[var(--muted)]">
              仅删除本机登录凭据，不影响云端文件。
            </div>
          </div>
          <Button variant="outline" size="sm" onClick={() => setLogoutOpen(true)}>
            <LogOut className="h-3.5 w-3.5" />
            退出
          </Button>
        </section>

        <div className="flex justify-end pb-2">
          <Button size="lg" onClick={save} disabled={saving}>
            {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <ArchiveRestore className="h-4 w-4" />}
            保存设置
          </Button>
        </div>
      </div>

      <Dialog
        open={logoutOpen}
        onOpenChange={setLogoutOpen}
        title="退出当前账号？"
        description="退出后需要重新扫码登录。云端文件不会被删除。"
        footer={
          <>
            <Button variant="ghost" onClick={() => setLogoutOpen(false)}>
              取消
            </Button>
            <Button variant="danger" onClick={logout} disabled={loggingOut}>
              {loggingOut ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
              退出登录
            </Button>
          </>
        }
      />
    </div>
  );
}

function SectionTitle({ icon: Icon, title }: { icon: typeof Sun; title: string }) {
  return (
    <div className="flex items-center gap-2">
      <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--accent-soft)] text-[var(--accent)]">
        <Icon className="h-3.5 w-3.5" />
      </div>
      <h2 className="text-[13px] font-semibold text-[var(--ink)]">{title}</h2>
    </div>
  );
}

function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-xl bg-[var(--panel-2)] px-3 py-2.5">
      <span className="text-[var(--muted)]">{label}</span>
      <span className={cn("truncate text-[var(--ink)]", mono && "font-mono text-[10px]")}>
        {value}
      </span>
    </div>
  );
}
