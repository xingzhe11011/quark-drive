import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatBytes(bytes: number, digits = 1): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : digits)} ${units[unit]}`;
}

export function formatSpeed(bytesPerSecond: number): string {
  if (!bytesPerSecond || bytesPerSecond < 1) return "—";
  return `${formatBytes(bytesPerSecond)}/s`;
}

export function formatDate(timestamp: number): string {
  if (!timestamp) return "—";
  const date = new Date(timestamp);
  const now = new Date();
  const sameYear = date.getFullYear() === now.getFullYear();
  return date.toLocaleString("zh-CN", {
    year: sameYear ? undefined : "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatDateOnly(timestamp: number): string {
  if (!timestamp) return "—";
  return new Date(timestamp).toLocaleDateString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });
}

export function vipLabel(type?: string | null): string {
  const kind = (type || "").toUpperCase();
  if (!kind) return "普通用户";
  if (kind.includes("SVIP") || kind.includes("SUPER")) return "超级会员";
  if (kind.includes("VIP") || kind.includes("MEMBER")) return "会员";
  return type || kind;
}

export function percent(done: number, total: number): number {
  if (!total) return 0;
  return Math.max(0, Math.min(100, (done / total) * 100));
}

export function phaseLabel(phase: string): string {
  const map: Record<string, string> = {
    queued: "排队中",
    hashing: "校验文件",
    preparing: "准备中",
    uploading: "上传中",
    downloading: "下载中",
    committing: "合并分片",
    done: "已完成",
    failed: "失败",
    paused: "已暂停",
    canceled: "已取消",
  };
  return map[phase] || phase;
}

export function statusTone(status: string): "accent" | "success" | "danger" | "muted" {
  if (status === "done") return "success";
  if (status === "failed") return "danger";
  if (["running", "preparing", "committing"].includes(status)) return "accent";
  return "muted";
}
