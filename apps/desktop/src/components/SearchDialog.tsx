import { useEffect, useState } from "react";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { t, locale } from "@/i18n";
import { type PathRow, api } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { NodeIcon } from "./NodeIcon";

const LIMIT = 100;
const DELAY_MS = 120;

interface Props {
  open: boolean;
  scanned: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (node: number) => void;
}

export function SearchDialog({ open, scanned, onOpenChange, onPick }: Props) {
  const [query, setQuery] = useState("");
  const [rows, setRows] = useState<PathRow[]>([]);

  useEffect(() => {
    if (!scanned || query.trim() === "") {
      setRows([]);
      return;
    }
    let live = true;
    const timer = window.setTimeout(() => {
      void api.search(query.trim(), LIMIT).then((page) => {
        if (live) setRows(page.rows);
      });
    }, DELAY_MS);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [query, scanned]);

  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("search.open")}
      description={t("search.placeholder")}
      className="sm:max-w-2xl"
    >
      {/* The search runs in Rust, so the list must not filter again. */}
      <Command shouldFilter={false}>
        <CommandInput
          value={query}
          onValueChange={setQuery}
          placeholder={t("search.placeholder")}
        />
        <CommandList>
          <CommandEmpty>{t(scanned ? "search.empty" : "search.needScan")}</CommandEmpty>
          {rows.map((row) => (
            <CommandItem
              key={row.id}
              value={String(row.id)}
              onSelect={() => {
                onPick(row.id);
                onOpenChange(false);
              }}
            >
              <NodeIcon category={row.category} />
              <div className="min-w-0 flex-1">
                <div className="truncate text-xs">{row.name}</div>
                <div className="truncate text-[11px] text-muted-foreground">{row.path}</div>
              </div>
              <span className="text-xs text-muted-foreground tabular-nums">
                {formatBytes(row.allocated, locale)}
              </span>
            </CommandItem>
          ))}
        </CommandList>
      </Command>
    </CommandDialog>
  );
}
