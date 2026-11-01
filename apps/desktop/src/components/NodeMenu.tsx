import { Copy, ExternalLink, Info, Maximize2, Trash2 } from "lucide-react";
import {
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
} from "@/components/ui/context-menu";
import { type TextKey, t } from "@/i18n";
import { FLAG_SYNTHETIC, type Row, isDirectory } from "@/lib/api";

interface Props {
  row: Row | null;
  onOpen: (row: Row) => void;
  onReveal: (row: Row) => void;
  onCopyPath: (row: Row) => void;
  onProperties: (row: Row) => void;
  onRecycle: (row: Row) => void;
}

/** The same actions for the tree, the lists and the treemap. */
export function NodeMenu({ row, onOpen, onReveal, onCopyPath, onProperties, onRecycle }: Props) {
  if (!row) {
    return <ContextMenuContent />;
  }
  const onDisk = (row.flags & FLAG_SYNTHETIC) === 0;
  return (
    <ContextMenuContent className="min-w-52">
      {isDirectory(row) && (
        <ContextMenuItem onSelect={() => onOpen(row)}>
          <Maximize2 />
          {t("action.open")}
        </ContextMenuItem>
      )}
      <ContextMenuItem disabled={!onDisk} onSelect={() => onReveal(row)}>
        <ExternalLink />
        {t("action.reveal")}
      </ContextMenuItem>
      <ContextMenuItem disabled={!onDisk} onSelect={() => onCopyPath(row)}>
        <Copy />
        {t("action.copyPath")}
      </ContextMenuItem>
      <ContextMenuItem disabled={!onDisk} onSelect={() => onProperties(row)}>
        <Info />
        {t("action.properties")}
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem
        variant="destructive"
        disabled={row.refusal !== null}
        onSelect={() => onRecycle(row)}
      >
        <Trash2 />
        {t("action.recycle")}
      </ContextMenuItem>
      {row.refusal !== null && (
        <p className="max-w-60 px-2 py-1 text-[11px] text-muted-foreground">
          {t(`refused.${row.refusal}` as TextKey)}
        </p>
      )}
    </ContextMenuContent>
  );
}
