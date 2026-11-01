import { Channel, invoke } from "@tauri-apps/api/core";

export const ROOT = 0;

export const FLAG_DIRECTORY = 1 << 0;
export const FLAG_SYNTHETIC = 1 << 1;
export const FLAG_METAFILE = 1 << 2;
export const FLAG_HARD_LINK = 1 << 3;
export const FLAG_LINK = 1 << 4;
export const FLAG_CLOUD = 1 << 5;
export const FLAG_STREAMS = 1 << 8;

export interface Volume {
  letter: string;
  label: string;
  fileSystem: string;
  kind: "fixed" | "removable" | "network" | "optical" | "memory" | "unknown";
  totalBytes: number;
  freeBytes: number;
}

export interface AppInfo {
  version: string;
  elevated: boolean;
  mica: boolean;
  /** Drive to scan right away, after reopening with administrator rights. */
  autoScan: string | null;
}

export interface ScanSummary {
  mode: "mft" | "walk";
  usedBytes: number | null;
  totalBytes: number | null;
  problems: number;
  allocatedBytes: number;
  logicalBytes: number;
  files: number;
  entries: number;
  seconds: number;
}

export type ScanEvent =
  | { kind: "progress"; done: number; total: number }
  | { kind: "finished"; summary: ScanSummary }
  | { kind: "cancelled" }
  | { kind: "failed"; message: string };

export type Category =
  | "directory"
  | "other"
  | "document"
  | "image"
  | "video"
  | "audio"
  | "archive"
  | "code"
  | "application"
  | "system";

export interface Row {
  id: number;
  name: string;
  logical: number;
  allocated: number;
  files: number;
  modified: number | null;
  flags: number;
  category: Category;
  hasChildren: boolean;
  /** Share of the parent's space, from 0 to 1. */
  share: number;
  /** Why this entry cannot go to the Recycle Bin, if it cannot. */
  refusal: string | null;
}

export interface RowPage {
  rows: Row[];
  total: number;
}

export interface PathRow extends Row {
  /** The folder that holds this entry. */
  path: string;
}

export interface PathRowPage {
  rows: PathRow[];
  total: number;
}

export interface TypeRow {
  extension: string;
  category: Category;
  files: number;
  logical: number;
  allocated: number;
}

export type SortColumn = "allocated" | "logical" | "name" | "files" | "modified";

export interface Refused {
  id: number;
  name: string;
  reason: string;
}

export interface RecycleResult {
  removed: number[];
  freedBytes: number;
  refused: Refused[];
  aborted: boolean;
}

export const isDirectory = (row: Row) => (row.flags & FLAG_DIRECTORY) !== 0;

export const api = {
  listVolumes: () => invoke<Volume[]>("list_volumes"),
  appInfo: () => invoke<AppInfo>("app_info"),
  startScan: (letter: string, onEvent: (event: ScanEvent) => void) => {
    const channel = new Channel<ScanEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("start_scan", { letter, onEvent: channel });
  },
  cancelScan: () => invoke<void>("cancel_scan"),
  getNode: (node: number) => invoke<Row>("get_node", { node }),
  getAncestry: (node: number) => invoke<Row[]>("get_ancestry", { node }),
  getPath: (node: number) => invoke<string>("get_path", { node }),
  countOtherLinks: (node: number) => invoke<number>("count_other_links", { node }),
  getChildren: (
    node: number,
    sort: SortColumn,
    descending: boolean,
    offset: number,
    limit: number,
  ) => invoke<RowPage>("get_children", { node, sort, descending, offset, limit }),
  getTreemap: (node: number, width: number, height: number, maxTiles: number) =>
    invoke<ArrayBuffer>("get_treemap", { node, width, height, maxTiles }),
  topFiles: (extension: string | null, offset: number, limit: number) =>
    invoke<PathRowPage>("top_files", { extension, offset, limit }),
  fileTypes: () => invoke<TypeRow[]>("file_types"),
  search: (query: string, limit: number) => invoke<PathRowPage>("search", { query, limit }),
  revealInExplorer: (node: number) => invoke<void>("reveal_in_explorer", { node }),
  copyPath: (node: number) => invoke<void>("copy_path", { node }),
  showProperties: (node: number) => invoke<void>("show_properties", { node }),
  recycle: (nodes: number[]) => invoke<RecycleResult>("recycle", { nodes }),
  relaunchElevated: (letter: string | null) => invoke<void>("relaunch_elevated", { letter }),
};
