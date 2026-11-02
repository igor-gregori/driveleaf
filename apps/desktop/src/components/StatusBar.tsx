import { Rabbit, Turtle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { t, locale } from "@/i18n";
import type { TextKey } from "@/i18n";
import type { Category, Row, ScanSummary } from "@/lib/api";
import { formatBytes, formatCount, formatSeconds, formatShare } from "@/lib/format";

const LEGEND: Category[] = [
  "document",
  "image",
  "video",
  "audio",
  "archive",
  "code",
  "application",
  "system",
  "other",
];

interface Props {
  shown: Row | null;
  summary: ScanSummary | null;
}

export function StatusBar({ shown, summary }: Props) {
  const fast = summary?.mode === "mft";
  const ModeIcon = fast ? Rabbit : Turtle;
  return (
    <footer className="flex h-7 shrink-0 items-center gap-3 border-t px-3 text-[11px] text-muted-foreground">
      <div className="flex min-w-0 flex-1 items-center gap-2">
        {shown ? (
          <>
            <span className="truncate text-foreground">{shown.name}</span>
            <span className="shrink-0 tabular-nums">
              {formatBytes(shown.allocated, locale)} · {formatShare(shown.share, locale)}
            </span>
          </>
        ) : (
          summary && (
            <span className="truncate">
              {t("scan.done", {
                files: formatCount(summary.files, locale),
                seconds: formatSeconds(summary.seconds, locale),
              })}
            </span>
          )
        )}
      </div>
      {summary && (
        <div className="hidden items-center gap-2 lg:flex">
          {LEGEND.map((category) => (
            <span key={category} className="flex items-center gap-1">
              <span
                className="size-2 rounded-[2px]"
                style={{ background: `var(--treemap-${category})` }}
              />
              {t(`category.${category}` as TextKey)}
            </span>
          ))}
        </div>
      )}
      {summary && summary.problems > 0 && (
        <span>{t("mode.problems", { count: formatCount(summary.problems, locale) })}</span>
      )}
      {summary && (
        <Tooltip>
          <TooltipTrigger asChild>
            <Badge variant={fast ? "secondary" : "outline"} className="h-5 gap-1 text-[10px]">
              <ModeIcon className="size-3" />
              {t(fast ? "mode.fast" : "mode.slow")}
            </Badge>
          </TooltipTrigger>
          <TooltipContent>{t(fast ? "mode.fastHint" : "mode.slowHint")}</TooltipContent>
        </Tooltip>
      )}
    </footer>
  );
}
