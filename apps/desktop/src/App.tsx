import { useEffect, useState } from "react";
import { toast } from "sonner";
import { backend, subscribeEvents } from "./lib/backend";
import type { Account } from "./lib/types";
import { useAppStore } from "./store";
import LoginPage from "./pages/LoginPage";
import MainShell from "./pages/MainShell";

function Splash() {
  return (
    <div className="mesh-bg flex h-full items-center justify-center bg-[var(--bg)]">
      <div className="flex flex-col items-center gap-4">
        <div className="relative flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--accent)] shadow-[0_18px_50px_-18px_var(--accent)]">
          <div className="h-6 w-6 rounded-full border-[3px] border-white/90" />
          <div className="absolute inset-0 animate-ping rounded-2xl bg-[var(--accent)]/20" />
        </div>
        <div className="text-sm font-medium text-[var(--muted)]">正在打开网盘…</div>
      </div>
    </div>
  );
}

export default function App() {
  const [loading, setLoading] = useState(true);
  const account = useAppStore((state) => state.account);
  const settings = useAppStore((state) => state.settings);
  const setAccount = useAppStore((state) => state.setAccount);
  const setSettings = useAppStore((state) => state.setSettings);
  const setDataDir = useAppStore((state) => state.setDataDir);
  const setTransfers = useAppStore((state) => state.setTransfers);
  const upsertTransfer = useAppStore((state) => state.upsertTransfer);
  const patchTransfer = useAppStore((state) => state.patchTransfer);

  useEffect(() => {
    let cancelled = false;
    const init = async () => {
      try {
        const data = await backend.bootstrap();
        if (cancelled) return;
        setSettings(data.settings);
        setDataDir(data.dataDir);
        setTransfers(data.transfers);

        const stored = data.accounts[0] ?? null;
        if (!stored) {
          setAccount(null);
          return;
        }

        try {
          const account = await backend.refreshAccount(stored.id);
          if (!cancelled) setAccount(account);
        } catch (error) {
          if (cancelled) return;
          const message = String(error).toLowerCase();
          const authExpired =
            message.includes("login") ||
            message.includes("cookie") ||
            message.includes("401") ||
            message.includes("403");
          if (authExpired) {
            setAccount(null);
            toast("登录已过期，请重新扫码");
          } else {
            // Keep the cached account when the network is temporarily down.
            setAccount(stored);
            toast.error(`账号校验失败，已使用本地会话：${error}`);
          }
        }
      } catch (error) {
        toast.error(`初始化失败：${error}`);
      } finally {
        if (!cancelled) setLoading(false);
      }
    };
    void init();
    return () => {
      cancelled = true;
    };
  }, [setAccount, setDataDir, setSettings, setTransfers]);

  useEffect(() => {
    let dispose: () => void = () => undefined;
    subscribeEvents((event) => {
      switch (event.type) {
        case "accountsChanged":
          setAccount(event.accounts[0] ?? null);
          break;
        case "transferChanged":
          upsertTransfer(event.transfer);
          break;
        case "transferProgress":
          patchTransfer(event.id, {
            status: event.status,
            phase: event.phase,
            doneBytes: event.doneBytes,
            totalBytes: event.totalBytes,
            speedBps: event.speedBps,
          });
          break;
        case "toast":
          if (event.level === "error") toast.error(event.message);
          else toast(event.message);
          break;
        default:
          break;
      }
    }).then((unlisten) => {
      dispose = unlisten;
    });
    return () => dispose();
  }, [patchTransfer, setAccount, upsertTransfer]);

  useEffect(() => {
    const root = document.documentElement;
    const dark =
      settings.theme === "dark" ||
      (settings.theme === "system" &&
        window.matchMedia("(prefers-color-scheme: dark)").matches);
    root.classList.toggle("dark", dark);
  }, [settings.theme]);

  if (loading) return <Splash />;

  if (!account) {
    return (
      <LoginPage
        onLogin={(next: Account) => {
          setAccount(next);
          void backend.listTransfers().then(setTransfers);
          void backend
            .refreshAccount(next.id)
            .then(setAccount)
            .catch((error) => toast.error(`账号资料刷新失败：${error}`));
        }}
      />
    );
  }

  return <MainShell />;
}
