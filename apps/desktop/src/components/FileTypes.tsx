import { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { t, locale } from "@/i18n";
import { type TypeRow, api } from "@/lib/api";
import { formatBytes, formatCount } from "@/lib/format";
import { NodeIcon } from "./NodeIcon";
import { ShareBar } from "./ShareBar";

const ROW_HEIGHT = 30;

interface Props {
  generation: number;
  scanId: number;
  onPick: (extension: string) => void;
}

export function FileTypes({ generation, scanId, onPick }: Props) {
  const [rows, setRows] = useState<TypeRow[]>([]);
  const scroller = useRef<HTMLDivElement>(null);
  const whole = useMemo(() => rows.reduce((sum, row) => sum + row.allocated, 0), [rows]);
  const virtual = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  useEffect(() => {
    let live = true;
    void api.fileTypes().then((result) => {
      if (live) setRows(result);
    });
    return () => {
      live = false;
    };
  }, [generation, scanId]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="grid grid-cols-[minmax(0,1fr)_56px_60px_76px] gap-2 border-b px-2 py-1 text-[11px] font-medium text-muted-foreground">
        <span>{t("column.extension")}</span>
        <span>{t("column.share")}</span>
        <span className="text-right">{t("column.files")}</span>
        <span className="text-right">{t("column.onDisk")}</span>
      </div>
      <div ref={scroller} className="min-h-0 flex-1 overflow-auto">
        <div className="relative w-full" style={{ height: virtual.getTotalSize() }}>
          {virtual.getVirtualItems().map((item) => {
            const row = rows[item.index];
            return (
              <button
                key={row.extension}
                type="button"
                className="absolute inset-x-0 grid grid-cols-[minmax(0,1fr)_56px_60px_76px] items-center gap-2 px-2 text-left text-xs hover:bg-accent/60"
                style={{ height: ROW_HEIGHT, transform: `translateY(${item.start}px)` }}
                onClick={() => onPick(row.extension)}
              >
                <span className="flex min-w-0 items-center gap-1.5">
                  <NodeIcon category={row.category} />
                  <span className="truncate">
                    {row.extension === "" ? t("types.none") : `.${row.extension}`}
                  </span>
                </span>
                <ShareBar share={whole === 0 ? 0 : row.allocated / whole} />
                <span className="text-right text-muted-foreground tabular-nums">
                  {formatCount(row.files, locale)}
                </span>
                <span className="text-right tabular-nums">{formatBytes(row.allocated, locale)}</span>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
