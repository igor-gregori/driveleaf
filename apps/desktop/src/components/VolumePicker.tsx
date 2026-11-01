import { HardDrive, Leaf, Usb, Network, Disc } from "lucide-react";
import { Progress } from "@/components/ui/progress";
import { t, locale } from "@/i18n";
import type { Volume } from "@/lib/api";
import { formatBytes } from "@/lib/format";

const ICONS: Record<Volume["kind"], typeof HardDrive> = {
  fixed: HardDrive,
  removable: Usb,
  network: Network,
  optical: Disc,
  memory: HardDrive,
  unknown: HardDrive,
};

interface Props {
  volumes: Volume[];
  onPick: (letter: string) => void;
}

/** What the main area shows before the first scan. */
export function VolumePicker({ volumes, onPick }: Props) {
  return (
    <div className="grid size-full place-items-center p-8">
      <div className="w-full max-w-lg">
        <Leaf className="mb-3 size-8 text-primary" aria-hidden />
        <h1 className="text-lg font-semibold">{t("scan.pickTitle")}</h1>
        <p className="mb-5 text-sm text-muted-foreground">{t("scan.pickHint")}</p>
        <div className="flex flex-col gap-2">
          {volumes.map((volume) => {
            const Icon = ICONS[volume.kind];
            const used = volume.totalBytes - volume.freeBytes;
            return (
              <button
                key={volume.letter}
                type="button"
                className="flex items-center gap-3 rounded-lg border bg-card/60 p-3 text-left transition-colors hover:bg-accent"
                onClick={() => onPick(volume.letter)}
              >
                <Icon className="size-5 shrink-0 text-muted-foreground" />
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline justify-between gap-2 text-sm">
                    <span className="truncate font-medium">
                      {volume.letter}: {volume.label || t("volume.local")}
                    </span>
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {volume.fileSystem}
                    </span>
                  </div>
                  <Progress
                    className="my-1.5 h-1.5"
                    value={volume.totalBytes === 0 ? 0 : (used / volume.totalBytes) * 100}
                  />
                  <div className="text-xs text-muted-foreground">
                    {t("volume.free", {
                      free: formatBytes(volume.freeBytes, locale),
                      total: formatBytes(volume.totalBytes, locale),
                    })}
                  </div>
                </div>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
