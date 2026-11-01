import {
  AppWindow,
  Archive,
  Code2,
  File,
  FileText,
  Film,
  Folder,
  Image,
  Music,
  Settings2,
} from "lucide-react";
import type { Category } from "@/lib/api";

const ICONS: Record<Category, typeof File> = {
  directory: Folder,
  other: File,
  document: FileText,
  image: Image,
  video: Film,
  audio: Music,
  archive: Archive,
  code: Code2,
  application: AppWindow,
  system: Settings2,
};

export function NodeIcon({ category }: { category: Category }) {
  const Icon = ICONS[category];
  return (
    <Icon
      className="size-3.5 shrink-0"
      style={{ color: `var(--treemap-${category === "directory" ? "other" : category})` }}
    />
  );
}
