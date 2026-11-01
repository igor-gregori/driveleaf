import { Cloud, Database, Layers, Link2, Unlink } from "lucide-react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { t } from "@/i18n";
import type { TextKey } from "@/i18n";
import {
  FLAG_CLOUD,
  FLAG_HARD_LINK,
  FLAG_LINK,
  FLAG_METAFILE,
  FLAG_STREAMS,
  type Row,
} from "@/lib/api";

const BADGES: [number, typeof Link2, TextKey][] = [
  [FLAG_HARD_LINK, Link2, "badge.hardLink"],
  [FLAG_LINK, Unlink, "badge.link"],
  [FLAG_CLOUD, Cloud, "badge.cloud"],
  [FLAG_METAFILE, Database, "badge.metafile"],
  [FLAG_STREAMS, Layers, "badge.streams"],
];

/** Small marks for entries whose size needs explaining. */
export function NodeBadges({ row }: { row: Row }) {
  return BADGES.filter(([flag]) => (row.flags & flag) !== 0).map(([flag, Icon, text]) => (
    <Tooltip key={flag}>
      <TooltipTrigger asChild>
        <Icon className="size-3 shrink-0 text-muted-foreground" />
      </TooltipTrigger>
      <TooltipContent>{t(text)}</TooltipContent>
    </Tooltip>
  ));
}
