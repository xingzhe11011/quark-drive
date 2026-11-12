import { useEffect, useRef, useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  Cloud,
  FolderSync,
  Loader2,
  LockKeyhole,
  RefreshCw,
  ScanLine,
  ShieldCheck,
  Sparkles,
  TerminalSquare,
  Zap,
} from "lucide-react";
import { QRCodeSVG } from "qrcode.react";
import { toast } from "sonner";
import type { Account } from "../lib/types";
import { backend, isDesktop } from "../lib/backend";
import { cn } from "../lib/utils";
import { Button } from "../components/ui/button";

type LoginStatus = "loading" | "waiting" | "success" | "expired" | "error";

function QrPanel({
  qrUrl,
  status,
  onRefresh,
}: {
  qrUrl: string;
  status: LoginStatus;
  onRefresh: () => void;
}) {
  const unavailable = status === "expired" || status === "error";

  return (
    <div className="relative mx-auto w-[228px] overflow-hidden rounded-[24px] bg-white p-3 pb-2 shadow-[0_20px_50px_-24px_rgba(20,20,50,0.45)] ring-1 ring-black/[0.035]">
      <div className="relative flex h-[202px] items-center justify-center">
        {qrUrl ? (
          <div
            className={cn(
              "transition duration-200",
              unavailable && "scale-[0.98] opacity-15 blur-[2px]",
            )}
          >
            <QRCodeSVG
              value={qrUrl}
              size={190}
              level="M"
              marginSize={1}
              bgColor="#ffffff"
              fgColor="#17192b"
            />
          </div>
        ) : status === "loading" ? (
          <div className="flex flex-col items-center gap-3 text-[#68708d]">
            <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-[#f4f3ff]">
              <Loader2 className="h-5 w-5 animate-spin text-[var(--accent)]" />
            </div>
            <span className="text-[11px] font-medium">正在生成二维码…</span>
          </div>
        ) : (
          <div className="flex flex-col items-center gap-3 px-8 text-center text-[#68708d]">
            <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-red-50">
              <AlertCircle className="h-5 w-5 text-red-500" />
            </div>
            <span className="text-[11px] font-medium">二维码获取失败</span>
          </div>
        )}

        {unavailable ? (
          <button
            onClick={onRefresh}
            className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-white/40 backdrop-blur-[1px]"
          >
            <span className="flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--accent)] text-white shadow-lg shadow-[var(--accent)]/25">
              <RefreshCw className="h-5 w-5" />
            </span>
            <span className="text-[12px] font-semibold text-[var(--ink)]">
              点击刷新二维码
            </span>
          </button>
        ) : null}
      </div>

      <div className="flex h-7 items-center justify-center gap-1.5 border-t border-[#f0f1f6] text-[10px] font-medium text-[#737892]">
        {status === "success" ? (
          <CheckCircle2 className="h-3.5 w-3.5 text-emerald-500" />
        ) : (
          <ScanLine className="h-3.5 w-3.5" />
        )}
        {status === "success" ? "登录成功" : "使用夸克 App 扫码"}
      </div>
    </div>
  );
}

export default function LoginPage({
  onLogin,
}: {
  onLogin: (account: Account) => void;
}) {
  const [status, setStatus] = useState<LoginStatus>("loading");
  const [message, setMessage] = useState("正在请求全新的登录二维码…");
  const [qrUrl, setQrUrl] = useState("");
  const [attempt, setAttempt] = useState(0);
  const [cookieBusy, setCookieBusy] = useState(false);
  const [showCookie, setShowCookie] = useState(false);
  const [cookie, setCookie] = useState("");
  const onLoginRef = useRef(onLogin);

  useEffect(() => {
    onLoginRef.current = onLogin;
  }, [onLogin]);

  useEffect(() => {
    let active = true;
    let sessionId: string | undefined;
    let timer: number | undefined;
    let failures = 0;

    const schedulePoll = (delay: number) => {
      timer = window.setTimeout(() => {
        void poll();
      }, delay);
    };

    const poll = async () => {
      if (!active || !sessionId) return;
      try {
        const result = await backend.pollQrLogin(sessionId);
        if (!active) return;
        failures = 0;

        if (result.state === "success" && result.account) {
          setStatus("success");
          setMessage("登录成功，正在进入网盘…");
          toast.success("登录成功");
          onLoginRef.current(result.account);
          return;
        }
        if (result.state === "expired") {
          setStatus("expired");
          setMessage(result.message || "二维码已过期，请刷新后重新扫码");
          return;
        }

        setStatus("waiting");
        setMessage("使用夸克 App 扫描二维码，并在手机上确认登录");
        schedulePoll(2000);
      } catch (error) {
        if (!active) return;
        failures += 1;
        if (failures < 3) {
          setStatus("waiting");
          setMessage(`网络波动，正在重试（${failures}/3）…`);
          schedulePoll(2000);
          return;
        }
        setStatus("error");
        setMessage(String(error));
        toast.error(`扫码登录失败：${error}`);
      }
    };

    const begin = async () => {
      setStatus("loading");
      setMessage("正在请求全新的登录二维码…");
      setQrUrl("");
      try {
        const started = await backend.startQrLogin();
        if (!active) {
          void backend.cancelQrLogin(started.sessionId);
          return;
        }
        sessionId = started.sessionId;
        setQrUrl(started.qrUrl);
        setStatus("waiting");
        setMessage("使用夸克 App 扫描二维码，并在手机上确认登录");
        schedulePoll(1200);
      } catch (error) {
        if (!active) return;
        setStatus("error");
        setMessage(String(error));
        toast.error(`获取二维码失败：${error}`);
      }
    };

    void begin();
    return () => {
      active = false;
      if (timer) window.clearTimeout(timer);
      if (sessionId) void backend.cancelQrLogin(sessionId);
    };
  }, [attempt]);

  function refreshQr() {
    setAttempt((value) => value + 1);
  }

  async function importCookie() {
    if (!cookie.trim()) {
      toast.error("请先粘贴 Cookie");
      return;
    }
    setCookieBusy(true);
    try {
      const account = await backend.importCookie(cookie.trim());
      toast.success("Cookie 验证通过");
      onLogin(account);
    } catch (error) {
      toast.error(`Cookie 验证失败：${error}`);
    } finally {
      setCookieBusy(false);
    }
  }

  async function enterDemo() {
    setCookieBusy(true);
    try {
      const account = await backend.importCookie("demo");
      toast.success("已进入演示模式");
      onLogin(account);
    } finally {
      setCookieBusy(false);
    }
  }

  return (
    <div className="grid h-full grid-cols-1 overflow-hidden bg-[var(--bg)] lg:grid-cols-[1.08fr_0.92fr]">
      <section className="brand-gradient relative hidden overflow-hidden lg:flex">
        <div className="absolute -left-24 top-24 h-72 w-72 rounded-full bg-white/8 blur-3xl" />
        <div className="absolute -right-16 bottom-8 h-80 w-80 rounded-full bg-cyan-300/10 blur-3xl" />
        <div className="relative z-10 flex w-full flex-col justify-between p-12 xl:p-16">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-white/12 ring-1 ring-white/20">
              <Cloud className="h-5 w-5 text-white" />
            </div>
            <div>
              <div className="text-[15px] font-semibold tracking-wide text-white">
                QuarkDrive
              </div>
              <div className="text-[11px] text-white/55">Linux Desktop</div>
            </div>
          </div>

          <div className="max-w-[520px]">
            <div className="mb-5 inline-flex items-center gap-2 rounded-full border border-white/15 bg-white/8 px-3 py-1.5 text-[11px] text-white/75 backdrop-blur">
              <Sparkles className="h-3.5 w-3.5 text-cyan-300" />
              为 Ubuntu / Linux 打造的夸克网盘客户端
            </div>
            <h1 className="text-[42px] font-semibold leading-[1.14] tracking-tight text-white xl:text-[52px]">
              把夸克网盘，
              <br />
              装进 Linux 桌面。
            </h1>
            <p className="mt-5 max-w-[460px] text-[14px] leading-7 text-white/62">
              二维码直接显示在登录窗口，扫码后自动完成验证。
              分片传输、断点续传、任务队列，一次配好，长期稳定。
            </p>

            <div className="mt-9 grid max-w-[500px] grid-cols-2 gap-3">
              {[
                { icon: ScanLine, title: "应用内扫码", desc: "不打开夸克网页" },
                { icon: Zap, title: "分片传输", desc: "多连接，速度更快" },
                { icon: FolderSync, title: "断点续传", desc: "中断重启不重来" },
                { icon: LockKeyhole, title: "本地存储", desc: "凭据仅存本机" },
              ].map(({ icon: Icon, title, desc }) => (
                <div
                  key={title}
                  className="rounded-2xl border border-white/10 bg-white/[0.055] p-4 backdrop-blur-sm"
                >
                  <Icon className="h-4.5 w-4.5 text-cyan-200" />
                  <div className="mt-3 text-[13px] font-medium text-white">
                    {title}
                  </div>
                  <div className="mt-1 text-[11px] text-white/48">{desc}</div>
                </div>
              ))}
            </div>
          </div>

          <div className="flex items-center gap-2 text-[11px] text-white/40">
            <ShieldCheck className="h-3.5 w-3.5" />
            第三方客户端 · 仅访问你自己的账号数据
          </div>
        </div>
      </section>

      <section className="mesh-bg relative flex items-center justify-center overflow-y-auto px-6 py-10">
        <div className="w-full max-w-[420px]">
          <div className="mb-7 text-center lg:hidden">
            <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--accent)] shadow-lg">
              <Cloud className="h-6 w-6 text-white" />
            </div>
            <div className="text-lg font-semibold">夸克网盘</div>
            <div className="text-xs text-[var(--muted)]">Linux Desktop</div>
          </div>

          <div className="rounded-[26px] border border-[var(--line)] bg-[var(--panel)]/92 p-7 shadow-[0_30px_80px_-36px_rgba(25,25,70,0.28)] backdrop-blur-xl">
            <div className="mb-6 text-center">
              <div className="text-[20px] font-semibold tracking-tight text-[var(--ink)]">
                扫码登录夸克网盘
              </div>
              <div className="mt-1.5 text-[12px] text-[var(--muted)]">
                每次登录都会生成一张新二维码
              </div>
            </div>

            <QrPanel qrUrl={qrUrl} status={status} onRefresh={refreshQr} />

            <div
              className={cn(
                "mt-5 flex min-h-[44px] items-center justify-center gap-2 rounded-xl px-3 py-2.5 text-center text-[11px] leading-5",
                status === "error"
                  ? "bg-red-500/[0.07] text-red-600 dark:text-red-400"
                  : status === "success"
                    ? "bg-emerald-500/[0.08] text-emerald-700 dark:text-emerald-400"
                    : "bg-[var(--panel-2)] text-[var(--muted)]",
              )}
            >
              {status === "loading" || status === "waiting" ? (
                <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-[var(--accent)]" />
              ) : status === "expired" || status === "error" ? (
                <AlertCircle className="h-3.5 w-3.5 shrink-0" />
              ) : (
                <CheckCircle2 className="h-3.5 w-3.5 shrink-0" />
              )}
              <span>{message}</span>
            </div>

            <button
              onClick={refreshQr}
              disabled={status === "loading"}
              className="mt-3 flex w-full items-center justify-center gap-1.5 text-[11px] text-[var(--muted)] transition hover:text-[var(--accent)] disabled:cursor-default disabled:opacity-45"
            >
              <RefreshCw className="h-3.5 w-3.5" />
              刷新二维码
            </button>

            {!isDesktop ? (
              <button
                className="mt-3 w-full text-center text-[11px] text-[var(--muted)] transition hover:text-[var(--accent)]"
                onClick={enterDemo}
                disabled={cookieBusy}
              >
                浏览器预览模式：跳过扫码进入演示界面
              </button>
            ) : null}

            <div className="mt-6 border-t border-[var(--line)] pt-4">
              <button
                onClick={() => setShowCookie((value) => !value)}
                className="flex w-full items-center justify-between text-[11px] text-[var(--muted)] transition hover:text-[var(--ink)]"
              >
                <span className="inline-flex items-center gap-1.5">
                  <TerminalSquare className="h-3.5 w-3.5" />
                  Cookie 导入（验收 / 备用）
                </span>
                <span>{showCookie ? "收起" : "展开"}</span>
              </button>
              {showCookie ? (
                <div className="mt-3 space-y-2">
                  <textarea
                    value={cookie}
                    onChange={(event) => setCookie(event.target.value)}
                    placeholder="__puus=...; __pus=...; __uid=..."
                    className="h-20 w-full resize-none rounded-xl border border-[var(--line)] bg-[var(--panel-2)] p-3 font-mono text-[11px] leading-5 text-[var(--ink)] outline-none transition placeholder:text-[var(--muted)]/50 focus:border-[var(--accent)]/50"
                  />
                  <Button
                    variant="secondary"
                    size="sm"
                    className="w-full"
                    onClick={importCookie}
                    disabled={cookieBusy}
                  >
                    {cookieBusy ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : null}
                    验证并登录
                  </Button>
                  <p className="text-[10px] leading-4 text-[var(--muted)]/70">
                    Cookie 只保存在本机 0600 权限文件中，不会上传。
                  </p>
                </div>
              ) : null}
            </div>
          </div>

          <div className="mt-5 flex items-center justify-center gap-2 text-[10px] text-[var(--muted)]/60">
            <ShieldCheck className="h-3 w-3" />
            登录信息仅存储在本机
            <span className="mx-1">·</span>
            v0.2.0
          </div>
        </div>
      </section>
    </div>
  );
}
