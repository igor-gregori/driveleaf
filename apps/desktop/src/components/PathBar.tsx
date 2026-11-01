import { Fragment, useEffect, useState } from "react";
import { ArrowUp } from "lucide-react";
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
import { Button } from "@/components/ui/button";
import { t } from "@/i18n";
import { type Row, api } from "@/lib/api";

interface Props {
  focus: number;
  generation: number;
  onOpen: (node: number) => void;
}

/** Where the treemap is, and the way back up. */
export function PathBar({ focus, generation, onOpen }: Props) {
  const [ancestry, setAncestry] = useState<Row[]>([]);

  useEffect(() => {
    let live = true;
    void api.getAncestry(focus).then((rows) => {
      if (live) setAncestry(rows);
    });
    return () => {
      live = false;
    };
  }, [focus, generation]);

  const parent = ancestry.at(-2);
  return (
    <div className="flex h-8 shrink-0 items-center gap-1 border-b px-2">
      <Button
        size="icon"
        variant="ghost"
        className="size-6"
        aria-label={t("action.up")}
        title={t("action.up")}
        disabled={!parent}
        onClick={() => parent && onOpen(parent.id)}
      >
        <ArrowUp className="size-3.5" />
      </Button>
      <Breadcrumb className="min-w-0 overflow-hidden">
        <BreadcrumbList className="flex-nowrap text-xs">
          {ancestry.map((row, position) => {
            const last = position === ancestry.length - 1;
            return (
              <Fragment key={row.id}>
                <BreadcrumbItem className="min-w-0">
                  {last ? (
                    <BreadcrumbPage className="truncate">{row.name}</BreadcrumbPage>
                  ) : (
                    <BreadcrumbLink asChild>
                      <button type="button" className="truncate" onClick={() => onOpen(row.id)}>
                        {row.name}
                      </button>
                    </BreadcrumbLink>
                  )}
                </BreadcrumbItem>
                {!last && <BreadcrumbSeparator />}
              </Fragment>
            );
          })}
        </BreadcrumbList>
      </Breadcrumb>
    </div>
  );
}
