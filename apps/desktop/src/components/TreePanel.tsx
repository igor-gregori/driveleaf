import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ChevronRight, ChevronDown, ArrowDown, ArrowUp } from "lucide-react";
import { t, locale } from "@/i18n";
import { ROOT, type Row, type SortColumn, api, isDirectory } from "@/lib/api";
import { formatBytes, formatCount } from "@/lib/format";
import { cn } from "@/lib/utils";
import { NodeBadges } from "./NodeBadges";
import { NodeIcon } from "./NodeIcon";
import { ShareBar } from "./ShareBar";

const PAGE = 2000;
const ROW_HEIGHT = 26;
const INDENT = 14;

interface Loaded {
  rows: Row[];
  total: number;
}

type FlatRow =
  | { kind: "node"; row: Row; depth: number }
  | { kind: "more"; parent: number; depth: number; shown: number; total: number };

function flatten(cache: Map<number, Loaded>, expanded: Set<number>): FlatRow[] {
  const flat: FlatRow[] = [];
  const visit = (node: number, depth: number) => {
    const loaded = cache.get(node);
    if (!loaded) return;
    for (const row of loaded.rows) {
      flat.push({ kind: "node", row, depth });
      if (expanded.has(row.id)) visit(row.id, depth + 1);
    }
    if (loaded.rows.length < loaded.total) {
      flat.push({ kind: "more", parent: node, depth, shown: loaded.rows.length, total: loaded.total });
    }
  };
  visit(ROOT, 0);
  return flat;
}

interface Props {
  scanId: number;
  generation: number;
  selected: number | null;
  onSelect: (node: number) => void;
  onOpen: (node: number) => void;
  onMenu: (node: number) => void;
  onRemove: (node: number) => void;
}

export function TreePanel({ scanId, generation, selected, onSelect, onOpen, onMenu, onRemove }: Props) {
  const [cache, setCache] = useState(new Map<number, Loaded>());
  const [expanded, setExpanded] = useState(new Set<number>());
  const [sort, setSort] = useState<{ column: SortColumn; descending: boolean }>({
    column: "allocated",
    descending: true,
  });
  const scroller = useRef<HTMLDivElement>(null);
  const scrollTarget = useRef<number | null>(null);

  const flat = useMemo(() => flatten(cache, expanded), [cache, expanded]);
  const virtual = useVirtualizer({
    count: flat.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 16,
  });

  const fetchChildren = useCallback(
    (node: number, limit = PAGE) =>
      api.getChildren(node, sort.column, sort.descending, 0, limit),
    [sort],
  );

  // A new scan means new node IDs, so nothing from the old one can stay.
  useEffect(() => {
    if (scanId === 0) return;
    let live = true;
    setExpanded(new Set());
    void api.getChildren(ROOT, "allocated", true, 0, PAGE).then((page) => {
      if (live) setCache(new Map([[ROOT, page]]));
    });
    return () => {
      live = false;
    };
  }, [scanId]);

  // Sizes change after a removal, and the order after a new sort.
  const reloadRef = useRef({ expanded, cache });
  reloadRef.current = { expanded, cache };
  useEffect(() => {
    if (scanId === 0) return;
    let live = true;
    const nodes = [ROOT, ...reloadRef.current.expanded];
    void Promise.all(
      nodes.map(async (node) => {
        const limit = Math.max(PAGE, reloadRef.current.cache.get(node)?.rows.length ?? 0);
        return [node, await fetchChildren(node, limit)] as const;
      }),
    ).then((pages) => {
      if (live) setCache(new Map(pages));
    });
    return () => {
      live = false;
    };
  }, [fetchChildren, generation, scanId]);

  const toggle = useCallback(
    async (row: Row) => {
      if (expanded.has(row.id)) {
        setExpanded((current) => {
          const next = new Set(current);
          next.delete(row.id);
          return next;
        });
        return;
      }
      if (!cache.has(row.id)) {
        const page = await fetchChildren(row.id);
        setCache((current) => new Map(current).set(row.id, page));
      }
      setExpanded((current) => new Set(current).add(row.id));
    },
    [cache, expanded, fetchChildren],
  );

  const showMore = async (parent: number, shown: number) => {
    const page = await fetchChildren(parent, shown + PAGE);
    setCache((current) => new Map(current).set(parent, page));
  };

  // Selecting something elsewhere opens the folders on the way to it.
  useEffect(() => {
    if (selected === null || selected === ROOT) return;
    if (flat.some((item) => item.kind === "node" && item.row.id === selected)) {
      const index = flat.findIndex((item) => item.kind === "node" && item.row.id === selected);
      virtual.scrollToIndex(index, { align: "auto" });
      return;
    }
    let live = true;
    void (async () => {
      const ancestry = await api.getAncestry(selected);
      const folders = ancestry.slice(0, -1);
      const pages = await Promise.all(
        folders.map(async (folder) => [folder.id, await fetchChildren(folder.id)] as const),
      );
      if (!live) return;
      setCache((current) => {
        const next = new Map(current);
        pages.forEach(([folder, page], position) => {
          const wanted = ancestry[position + 1];
          // A big folder can hold the target past the loaded page.
          const has = page.rows.some((row) => row.id === wanted.id);
          next.set(folder, has ? page : { ...page, rows: [...page.rows, wanted] });
        });
        return next;
      });
      setExpanded((current) => {
        const next = new Set(current);
        folders.forEach((folder) => next.add(folder.id));
        next.delete(ROOT);
        return next;
      });
      scrollTarget.current = selected;
    })();
    return () => {
      live = false;
    };
    // Only a new selection starts this; `flat` is checked at that moment.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected]);

  useEffect(() => {
    if (scrollTarget.current === null) return;
    const index = flat.findIndex(
      (item) => item.kind === "node" && item.row.id === scrollTarget.current,
    );
    if (index >= 0) {
      virtual.scrollToIndex(index, { align: "center" });
      scrollTarget.current = null;
    }
  }, [flat, virtual]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    const position = flat.findIndex((item) => item.kind === "node" && item.row.id === selected);
    const current = position >= 0 ? flat[position] : null;
    const move = (delta: number) => {
      for (let index = position + delta; index >= 0 && index < flat.length; index += delta) {
        const item = flat[index];
        if (item.kind === "node") {
          onSelect(item.row.id);
          return;
        }
      }
    };
    switch (event.key) {
      case "ArrowDown":
        move(1);
        break;
      case "ArrowUp":
        move(-1);
        break;
      case "ArrowRight":
        if (current?.kind === "node" && current.row.hasChildren && !expanded.has(current.row.id)) {
          void toggle(current.row);
        }
        break;
      case "ArrowLeft":
        if (current?.kind === "node" && expanded.has(current.row.id)) {
          void toggle(current.row);
        }
        break;
      case "Enter":
        if (current?.kind === "node" && isDirectory(current.row)) onOpen(current.row.id);
        break;
      case "Delete":
        if (current?.kind === "node") onRemove(current.row.id);
        break;
      default:
        return;
    }
    event.preventDefault();
  };

  const sortBy = (column: SortColumn) =>
    setSort((current) => ({
      column,
      descending: current.column === column ? !current.descending : column !== "name",
    }));

  const SortMark = ({ column }: { column: SortColumn }) => {
    if (sort.column !== column) return null;
    const Icon = sort.descending ? ArrowDown : ArrowUp;
    return <Icon className="size-3" />;
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="grid grid-cols-[minmax(0,1fr)_56px_60px_76px] gap-2 border-b px-2 py-1 text-[11px] font-medium text-muted-foreground">
        <button type="button" className="flex items-center gap-1" onClick={() => sortBy("name")}>
          {t("column.name")}
          <SortMark column="name" />
        </button>
        <span>{t("column.share")}</span>
        <button
          type="button"
          className="flex items-center justify-end gap-1"
          onClick={() => sortBy("files")}
        >
          {t("column.files")}
          <SortMark column="files" />
        </button>
        <button
          type="button"
          className="flex items-center justify-end gap-1"
          onClick={() => sortBy("allocated")}
        >
          {t("column.onDisk")}
          <SortMark column="allocated" />
        </button>
      </div>
      <div
        ref={scroller}
        role="tree"
        tabIndex={0}
        onKeyDown={onKeyDown}
        className="min-h-0 flex-1 overflow-auto outline-none"
      >
        <div className="relative w-full" style={{ height: virtual.getTotalSize() }}>
          {virtual.getVirtualItems().map((item) => {
            const entry = flat[item.index];
            if (entry.kind === "more") {
              return (
                <button
                  key={`more-${entry.parent}`}
                  type="button"
                  className="absolute inset-x-0 px-2 text-left text-xs text-muted-foreground hover:text-foreground"
                  style={{
                    height: ROW_HEIGHT,
                    transform: `translateY(${item.start}px)`,
                    paddingLeft: 8 + entry.depth * INDENT + 18,
                  }}
                  onClick={() => void showMore(entry.parent, entry.shown)}
                >
                  {t("tree.more", {
                    shown: formatCount(entry.shown, locale),
                    total: formatCount(entry.total, locale),
                  })}
                </button>
              );
            }
            const { row, depth } = entry;
            const isOpen = expanded.has(row.id);
            const Chevron = isOpen ? ChevronDown : ChevronRight;
            return (
              <div
                key={row.id}
                role="treeitem"
                aria-selected={row.id === selected}
                aria-expanded={row.hasChildren ? isOpen : undefined}
                className={cn(
                  "absolute inset-x-0 grid cursor-pointer grid-cols-[minmax(0,1fr)_56px_60px_76px] items-center gap-2 px-2 text-xs select-none hover:bg-accent/60",
                  row.id === selected && "bg-accent",
                )}
                style={{ height: ROW_HEIGHT, transform: `translateY(${item.start}px)` }}
                onClick={() => onSelect(row.id)}
                onDoubleClick={() => {
                  if (isDirectory(row)) onOpen(row.id);
                }}
                onContextMenu={() => onMenu(row.id)}
              >
                <div
                  className="flex min-w-0 items-center gap-1"
                  style={{ paddingLeft: depth * INDENT }}
                >
                  {row.hasChildren ? (
                    <button
                      type="button"
                      aria-label={row.name}
                      className="grid size-4 shrink-0 place-items-center rounded-sm hover:bg-muted"
                      onClick={(event) => {
                        event.stopPropagation();
                        void toggle(row);
                      }}
                    >
                      <Chevron className="size-3" />
                    </button>
                  ) : (
                    <span className="size-4 shrink-0" />
                  )}
                  <NodeIcon category={row.category} />
                  <span className="truncate">{row.name}</span>
                  <NodeBadges row={row} />
                </div>
                <ShareBar share={row.share} />
                <span className="text-right text-muted-foreground tabular-nums">
                  {isDirectory(row) ? formatCount(row.files, locale) : ""}
                </span>
                <span className="text-right tabular-nums">{formatBytes(row.allocated, locale)}</span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
