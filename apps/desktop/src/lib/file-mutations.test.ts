import { describe, expect, it } from "vitest";
import { insertFile, removeFiles, renameFile } from "./file-mutations";
import type { FileItem, ListResult } from "./types";

function file(overrides: Partial<FileItem> = {}): FileItem {
  return {
    fid: "file-one",
    parentFid: "0",
    name: "文件一",
    isDir: false,
    size: 1,
    category: 0,
    formatType: "txt",
    updatedAt: 0,
    thumbnail: "",
    ...overrides,
  };
}

function listing(entries: FileItem[]): ListResult {
  return { entries, page: 1, size: 200, total: entries.length };
}

describe("file list mutations", () => {
  it("inserts a new file immediately and is idempotent", () => {
    const existing = listing([file()]);
    const created = file({ fid: "folder-new", name: "新文件夹", isDir: true });
    const inserted = insertFile(existing, created);
    expect(inserted?.entries.map((entry) => entry.fid)).toEqual([
      "folder-new",
      "file-one",
    ]);
    expect(inserted?.total).toBe(2);
    expect(insertFile(inserted, created)).toBe(inserted);
  });

  it("renames only the target entry and leaves unchanged data untouched", () => {
    const existing = listing([file(), file({ fid: "file-two", name: "文件二" })]);
    const renamed = renameFile(existing, "file-one", "新名称");
    expect(renamed?.entries[0].name).toBe("新名称");
    expect(renamed?.entries[1]).toEqual(existing.entries[1]);
    expect(renameFile(renamed, "file-one", "新名称")).toBe(renamed);
  });

  it("removes deleted entries and corrects the total once", () => {
    const existing = listing([
      file(),
      file({ fid: "file-two", name: "文件二" }),
    ]);
    const removed = removeFiles(existing, ["file-one"]);
    expect(removed?.entries.map((entry) => entry.fid)).toEqual(["file-two"]);
    expect(removed?.total).toBe(1);
    expect(removeFiles(removed, ["file-one"])).toBe(removed);
  });
});
