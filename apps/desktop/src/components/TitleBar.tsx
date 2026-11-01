import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Leaf, Minus, Search, Square, X } from "lucide-react";
import { Button } from "@/components/ui/button";
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
  onLetter: (letter: string) => void;
  onScan: () => void;
  onCancel: () => void;
  onSearch: () => void;
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
          ? "grid h-full w-11 place-items-center hover:bg-red-600 hover:text-white"
          : "grid h-full w-11 place-items-center hover:bg-accent"
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
  onLetter,
  onScan,
  onCancel,
  onSearch,
}: Props) {
  const appWindow = getCurrentWindow();
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
        <kbd className="ml-1 text-[10px] tracking-wide">Ctrl K</kbd>
      </Button>
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
