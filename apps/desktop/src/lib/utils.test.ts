import { describe, expect, it } from "vitest";
import {
  formatBytes,
  formatSpeed,
  percent,
  phaseLabel,
  vipLabel,
} from "./utils";

describe("format helpers", () => {
  it("formats byte sizes", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1024 ** 3 * 2.5)).toBe("2.5 GB");
  });

  it("formats transfer speed", () => {
    expect(formatSpeed(0)).toBe("—");
    expect(formatSpeed(1024 * 1024 * 3)).toBe("3.0 MB/s");
  });

  it("clamps progress percentages", () => {
    expect(percent(50, 100)).toBe(50);
    expect(percent(150, 100)).toBe(100);
    expect(percent(10, 0)).toBe(0);
  });

  it("maps phase labels", () => {
    expect(phaseLabel("hashing")).toBe("校验文件");
    expect(phaseLabel("unknown")).toBe("unknown");
  });

  it("maps Quark membership types", () => {
    expect(vipLabel("EXP_SVIP")).toBe("超级会员");
    expect(vipLabel("SUPER_VIP")).toBe("超级会员");
    expect(vipLabel("VIP")).toBe("会员");
    expect(vipLabel("")).toBe("普通用户");
  });
});
