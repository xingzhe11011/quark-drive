export type BackendErrorKind = "auth" | "privacy" | "other";

export function classifyBackendError(error: unknown): BackendErrorKind {
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  const detail = message.replace(/^protocol error: /, "");
  if (
    detail === "login session is missing or expired" ||
    detail.startsWith("invalid cookie: ")
  ) {
    return "auth";
  }

  const api = /^quark api error (\d{3})\/(-?\d+): ([\s\S]*)$/.exec(detail);
  if (!api) return "other";
  const status = Number(api[1]);
  const code = Number(api[2]);
  if (status === 403 && code === 24005) return "privacy";
  if (
    status === 401 ||
    [11001, 31001, 31002, 31003].includes(code) ||
    (status === 403 && /^(?:login required|login expired|not logged in)[.!]?$/i.test(api[3]))
  ) {
    return "auth";
  }
  return "other";
}

export function fileListErrorState(error: unknown) {
  const kind = classifyBackendError(error);
  if (kind === "privacy") {
    return {
      title: "隐私空间需要单独解锁",
      description: "请从左侧“隐私空间”进入并输入隐私密码。普通网盘登录和文件不会受到影响。",
      canRetry: false,
    };
  }
  if (kind === "auth") {
    return {
      title: "登录已失效",
      description: "请前往设置中心退出当前账号，再重新扫码登录。",
      canRetry: false,
    };
  }
  return {
    title: "文件列表加载失败",
    description: "网络或服务暂时不可用，请检查网络连接后重试。",
    canRetry: true,
  };
}
