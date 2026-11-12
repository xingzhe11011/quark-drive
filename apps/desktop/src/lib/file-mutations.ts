import type { FileItem, ListResult } from "./types";

export function insertFile(
  previous: ListResult | undefined,
  item: FileItem,
): ListResult | undefined {
  if (!previous || previous.entries.some((entry) => entry.fid === item.fid)) {
    return previous;
  }
  return {
    ...previous,
    entries: [item, ...previous.entries],
    total: previous.total + 1,
  };
}

export function renameFile(
  previous: ListResult | undefined,
  fid: string,
  name: string,
): ListResult | undefined {
  if (!previous) return previous;
  let changed = false;
  const entries = previous.entries.map((entry) => {
    if (entry.fid !== fid || entry.name === name) return entry;
    changed = true;
    return { ...entry, name };
  });
  return changed ? { ...previous, entries } : previous;
}

export function removeFiles(
  previous: ListResult | undefined,
  fids: Iterable<string>,
): ListResult | undefined {
  if (!previous) return previous;
  const removed = new Set(fids);
  const entries = previous.entries.filter((entry) => !removed.has(entry.fid));
  if (entries.length === previous.entries.length) return previous;
  return {
    ...previous,
    entries,
    total: Math.max(0, previous.total - (previous.entries.length - entries.length)),
  };
}
