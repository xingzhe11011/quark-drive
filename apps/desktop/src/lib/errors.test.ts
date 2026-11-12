import { describe, expect, it } from "vitest";
import { classifyBackendError, fileListErrorState } from "./errors";

describe("backend error classification", () => {
  it.each([
    "quark api error 403/24005: secret space limit: get file vo",
    "protocol error: quark api error 403/24005: secret space limit: get file vo",
    new Error("protocol error: quark api error 403/24005: login to secret space"),
  ])("identifies only the structured privacy restriction: %s", (error) => {
    expect(classifyBackendError(error)).toBe("privacy");
  });

  it.each([
    "login session is missing or expired",
    "protocol error: login session is missing or expired",
    "protocol error: invalid cookie: missing authentication cookie",
    "quark api error 401/0: unauthorized",
    "protocol error: quark api error 401/14001: unauthorized",
    "quark api error 403/0: LOGIN required",
    "quark api error 403/14001: login expired",
    ...[11001, 31001, 31002, 31003].map((code) => `quark api error 200/${code}: session invalid`),
    new Error("protocol error: quark api error 401/31001: login required"),
  ])("preserves explicit login failures: %s", (error) => {
    expect(classifyBackendError(error)).toBe("auth");
  });

  it.each([
    "protocol error: quark api error 400/14001: Bad Parameter: [param missing!]",
    "quark api error 403/0: forbidden",
    "quark api error 403/0: cannot contact login service",
    "quark api error 500/24005: service unavailable",
    "network error: request to login service failed with 403",
    "io error: cannot read cookie file at /401/403",
    "unexpected response: quark api error 401/31001: login required",
    "secret space limit: get file vo",
    { status: 401, code: 31001 },
    undefined,
    null,
  ])("does not infer expired sessions from arbitrary text: %s", (error) => {
    expect(classifyBackendError(error)).toBe("other");
  });
});

describe("file list error state", () => {
  it("directs privacy restriction errors to the dedicated unlock page", () => {
    const state = fileListErrorState("protocol error: quark api error 403/24005: private-file.txt");
    expect(state.title).toBe("隐私空间需要单独解锁");
    expect(state.description).toContain("左侧“隐私空间”");
    expect(state.canRetry).toBe(false);
    expect(JSON.stringify(state)).not.toContain("private-file.txt");
  });

  it("shows reauthentication guidance for an expired session", () => {
    const state = fileListErrorState("protocol error: quark api error 401/31001: expired");
    expect(state.title).toBe("登录已失效");
    expect(state.description).toContain("重新扫码");
    expect(state.canRetry).toBe(false);
  });

  it("offers retry without displaying raw server content", () => {
    const state = fileListErrorState("network error: private-file.txt?token=sensitive");
    expect(state.title).toBe("文件列表加载失败");
    expect(state.canRetry).toBe(true);
    expect(JSON.stringify(state)).not.toContain("private-file.txt");
    expect(JSON.stringify(state)).not.toContain("sensitive");
  });
});
