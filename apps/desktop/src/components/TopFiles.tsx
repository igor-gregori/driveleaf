import { useEffect, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { t, locale } from "@/i18n";
import { type PathRow, api } from "@/lib/api";
import { formatBytes, formatDate } from "@/lib/format";
import { cn } from "@/lib/utils";
import { NodeBadges } from "./NodeBadges";
import { NodeIcon } from "./NodeIcon";

const PAGE = 200;
const ROW_HEIGHT = 40;
/** How close to the end of what is loaded before fetching more. */
const LOAD_AHEAD = 40;

interface Props {
  generation: number;
  scanId: number;
  extension: string | null;
  selected: number | null;
  onClearFilter: () => void;
  onSelect: (node: number) => void;
  onMenu: (node: number) => void;
}

export function TopFiles({
  generation,
  scanId,
  extension,
  selected,
  onClearFilter,
  onSelect,
  onMenu,
}: Props) {
  const [rows, setRows] = useState<PathRow[]>([]);
  const [total, setTotal] = useState(0);
  const loading = useRef(false);
  const scroller = useRef<HTMLDivElement>(null);
  const virtual = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  useEffect(() => {
    let live = true;
    loading.current = true;
    void api.topFiles(extension, 0, PAGE).then((page) => {
      loading.current = false;
      if (!live) return;
      setRows(page.rows);
      setTotal(page.total);
    });
    return () => {
      live = false;
    };
  }, [extension, generation, scanId]);

  const lastVisible = virtual.getVirtualItems().at(-1)?.index ?? 0;
  useEffect(() => {
    if (loading.current || rows.length >= total || lastVisible < rows.length - LOAD_AHEAD) return;
    loading.current = true;
    void api.topFiles(extension, rows.length, PAGE).then((page) => {
      loading.current = false;
      setRows((current) => [...current, ...page.rows]);
    });
  }, [extension, lastVisible, rows.length, total]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      {extension !== null && (
        <div className="flex items-center gap-2 border-b px-2 py-1">
          <Badge variant="secondary">{t("top.filter", { extension })}</Badge>
          <button
            type="button"
            aria-label={t("top.clearFilter")}
            className="ml-auto rounded-sm p-0.5 text-muted-foreground hover:bg-muted"
            onClick={onClearFilter}
          >
            <X className="size-3" />
          </button>
        </div>
      )}
      <div ref={scroller} className="min-h-0 flex-1 overflow-auto">
        <div className="relative w-full" style={{ height: virtual.getTotalSize() }}>
          {virtual.getVirtualItems().map((item) => {
            const row = rows[item.index];
            return (
              <div
                key={row.id}
                className={cn(
                  "absolute inset-x-0 flex cursor-default items-center gap-2 px-2 select-none hover:bg-accent/60",
                  row.id === selected && "bg-accent",
                )}
                style={{ height: ROW_HEIGHT, transform: `translateY(${item.start}px)` }}
                onClick={() => onSelect(row.id)}
                onContextMenu={() => onMenu(row.id)}
              >
                <NodeIcon category={row.category} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1 text-xs">
                    <span className="truncate">{row.name}</span>
                    <NodeBadges row={row} />
                  </div>
                  <div className="truncate text-[11px] text-muted-foreground">{row.path}</div>
                </div>
                <div className="shrink-0 text-right">
                  <div className="text-xs tabular-nums">{formatBytes(row.allocated, locale)}</div>
                  <div className="text-[11px] text-muted-foreground">
                    {formatDate(row.modified, locale)}
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
