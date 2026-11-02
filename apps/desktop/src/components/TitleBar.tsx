import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Leaf, Minus, Search, ShieldCheck, Square, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { t, locale } from "@/i18n";
import type { Volume } from "@/lib/api";
import { formatBytes } from "@/lib/format";

interface Props {
  volumes: Volume[];
  letter: string;
  scanning: boolean;
  scanned: boolean;
  elevated: boolean;
  onLetter: (letter: string) => void;
  onScan: () => void;
  onCancel: () => void;
  onSearch: () => void;
  onReopen: () => void;
}

function WindowButton({
  label,
  danger,
  onClick,
  children,
}: {
  label: string;
  danger?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={
        danger
          ? "grid h-full w-11 cursor-default place-items-center hover:bg-red-600 hover:text-white"
          : "grid h-full w-11 cursor-default place-items-center hover:bg-accent"
      }
      onClick={onClick}
    >
      {children}
    </button>
  );
}

export function TitleBar({
  volumes,
  letter,
  scanning,
  scanned,
  elevated,
  onLetter,
  onScan,
  onCancel,
  onSearch,
  onReopen,
}: Props) {
  const appWindow = getCurrentWindow();
  // Only NTFS has a fast mode to unlock.
  const canReopen =
    !elevated && volumes.find((volume) => volume.letter === letter)?.fileSystem === "NTFS";
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const update = () => void appWindow.isMaximized().then(setMaximized);
    update();
    const stop = appWindow.onResized(update);
    return () => {
      void stop.then((unlisten) => unlisten());
    };
  }, [appWindow]);

  return (
    <header data-tauri-drag-region className="flex h-10 shrink-0 items-center gap-2 border-b pl-3">
      <Leaf className="size-4 text-primary" aria-hidden />
      <span data-tauri-drag-region className="mr-2 text-sm font-semibold select-none">
        {t("app.name")}
      </span>
      <Select value={letter} onValueChange={onLetter} disabled={scanning}>
        <SelectTrigger size="sm" className="h-7 w-52 text-xs" aria-label={t("volume.label")}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {volumes.map((volume) => (
            <SelectItem key={volume.letter} value={volume.letter} className="text-xs">
              {volume.letter}: {volume.label || t("volume.local")} ·{" "}
              {formatBytes(volume.totalBytes, locale)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {scanning ? (
        <Button size="sm" variant="outline" className="h-7" onClick={onCancel}>
          {t("scan.cancel")}
        </Button>
      ) : (
        <Button size="sm" className="h-7" disabled={!letter} onClick={onScan}>
          {t(scanned ? "scan.again" : "scan.start")}
        </Button>
      )}
      <Button
        size="sm"
        variant="outline"
        className="h-7 gap-1.5 text-muted-foreground"
        onClick={onSearch}
      >
        <Search className="size-3.5" />
        {t("search.open")}
        <kbd className="ml-1 inline-flex h-4 items-center rounded border bg-muted px-1 font-sans text-[10px] leading-none font-medium">
          Ctrl K
        </kbd>
      </Button>
      {canReopen && (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              size="sm"
              variant="outline"
              className="h-7 gap-1.5 border-amber-500/60 bg-amber-500/10 text-amber-700 hover:bg-amber-500/20 hover:text-amber-800 dark:text-amber-300 dark:hover:text-amber-200"
              disabled={scanning}
              onClick={onReopen}
            >
              <ShieldCheck className="size-3.5" />
              {t("mode.reopen")}
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t("mode.reopenHint")}</TooltipContent>
        </Tooltip>
      )}
      <div data-tauri-drag-region className="h-full flex-1" />
      <WindowButton label={t("window.minimize")} onClick={() => void appWindow.minimize()}>
        <Minus className="size-3.5" />
      </WindowButton>
      <WindowButton label={t("window.maximize")} onClick={() => void appWindow.toggleMaximize()}>
        {maximized ? <Copy className="size-3" /> : <Square className="size-3" />}
      </WindowButton>
      <WindowButton label={t("window.close")} danger onClick={() => void appWindow.close()}>
        <X className="size-4" />
      </WindowButton>
    </header>
  );
}
