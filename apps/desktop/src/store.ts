import { create } from "zustand";
import type { Account, Settings, Transfer } from "./lib/types";

export type Page = "files" | "recycle" | "transfers" | "settings";
export type ViewMode = "list" | "grid";

export interface Crumb {
  fid: string;
  name: string;
}

interface AppStore {
  account: Account | null;
  settings: Settings;
  dataDir: string;
  transfers: Transfer[];
  page: Page;
  viewMode: ViewMode;
  crumbs: Crumb[];
  selection: string[];
  search: string;
  setAccount: (account: Account | null) => void;
  setSettings: (settings: Settings) => void;
  setDataDir: (dataDir: string) => void;
  setTransfers: (transfers: Transfer[]) => void;
  upsertTransfer: (transfer: Transfer) => void;
  patchTransfer: (
    id: string,
    patch: Partial<Transfer> & { speedBps?: number },
  ) => void;
  setPage: (page: Page) => void;
  setViewMode: (mode: ViewMode) => void;
  enterFolder: (fid: string, name: string) => void;
  jumpTo: (index: number) => void;
  setSelection: (selection: string[]) => void;
  toggleSelection: (fid: string) => void;
  clearSelection: () => void;
  setSearch: (search: string) => void;
  resetCrumbs: () => void;
}

const defaultSettings: Settings = {
  downloadDir: "",
  concurrency: 2,
  maxSpeedBps: 0,
  theme: "system",
};

export const useAppStore = create<AppStore>((set) => ({
  account: null,
  settings: defaultSettings,
  dataDir: "",
  transfers: [],
  page: "files",
  viewMode: "list",
  crumbs: [{ fid: "0", name: "我的网盘" }],
  selection: [],
  search: "",
  setAccount: (account) => set({ account }),
  setSettings: (settings) => set({ settings }),
  setDataDir: (dataDir) => set({ dataDir }),
  setTransfers: (transfers) => set({ transfers }),
  upsertTransfer: (transfer) =>
    set((state) => {
      const exists = state.transfers.some((item) => item.id === transfer.id);
      return {
        transfers: exists
          ? state.transfers.map((item) =>
              item.id === transfer.id ? transfer : item,
            )
          : [transfer, ...state.transfers],
      };
    }),
  patchTransfer: (id, patch) =>
    set((state) => ({
      transfers: state.transfers.map((item) =>
        item.id === id ? { ...item, ...patch } : item,
      ),
    })),
  setPage: (page) => set({ page }),
  setViewMode: (viewMode) => set({ viewMode }),
  enterFolder: (fid, name) =>
    set((state) => ({
      crumbs: [...state.crumbs, { fid, name }],
      selection: [],
      search: "",
    })),
  jumpTo: (index) =>
    set((state) => ({
      crumbs: state.crumbs.slice(0, index + 1),
      selection: [],
      search: "",
    })),
  setSelection: (selection) => set({ selection }),
  toggleSelection: (fid) =>
    set((state) => ({
      selection: state.selection.includes(fid)
        ? state.selection.filter((item) => item !== fid)
        : [...state.selection, fid],
    })),
  clearSelection: () => set({ selection: [] }),
  setSearch: (search) => set({ search }),
  resetCrumbs: () =>
    set({ crumbs: [{ fid: "0", name: "我的网盘" }], selection: [], search: "" }),
}));
