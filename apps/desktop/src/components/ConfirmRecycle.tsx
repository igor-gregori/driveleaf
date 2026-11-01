import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { t, locale } from "@/i18n";
import { type Row, isDirectory } from "@/lib/api";
import { formatBytes, formatCount } from "@/lib/format";

interface Props {
  row: Row | null;
  path: string;
  /** Other hard links keep the data on disk. */
  linked: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

/** Every removal goes through here. Nothing is ever deleted without this step. */
export function ConfirmRecycle({ row, path, linked, onCancel, onConfirm }: Props) {
  return (
    <AlertDialog
      open={row !== null}
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t("recycle.title")}</AlertDialogTitle>
          <AlertDialogDescription>
            {t("recycle.body", { name: row?.name ?? "" })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        {row && (
          <div className="rounded-md bg-muted px-3 py-2 text-xs">
            <div className="break-all">{path}</div>
            <div className="mt-1 text-muted-foreground">
              {t("recycle.size", { size: formatBytes(row.allocated, locale) })}
              {isDirectory(row) &&
                ` · ${t("recycle.files", { files: formatCount(row.files, locale) })}`}
            </div>
          </div>
        )}
        {linked && <p className="text-xs text-muted-foreground">{t("recycle.linked")}</p>}
        <AlertDialogFooter>
          <AlertDialogCancel>{t("recycle.cancel")}</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={onConfirm}>
            {t("recycle.confirm")}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
