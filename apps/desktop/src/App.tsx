import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { ConfirmRecycle } from "@/components/ConfirmRecycle";
import { FileTypes } from "@/components/FileTypes";
import { NodeMenu } from "@/components/NodeMenu";
import { PathBar } from "@/components/PathBar";
import { SearchDialog } from "@/components/SearchDialog";
import { StatusBar } from "@/components/StatusBar";
import { TitleBar } from "@/components/TitleBar";
import { TopFiles } from "@/components/TopFiles";
import { TreePanel } from "@/components/TreePanel";
import { TreemapView } from "@/components/TreemapView";
import { VolumePicker } from "@/components/VolumePicker";
import { ContextMenu, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Progress } from "@/components/ui/progress";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { t, locale } from "@/i18n";
import type { TextKey } from "@/i18n";
import {
  type AppInfo,
  ROOT,
  type RecycleResult,
  type Row,
  type ScanSummary,
  type Volume,
  api,
  isDirectory,
} from "@/lib/api";
import { formatBytes, formatCount } from "@/lib/format";

type ScanState =
  | { phase: "idle" }
  | { phase: "scanning"; done: number; total: number }
  | { phase: "done"; summary: ScanSummary };

function useSystemTheme() {
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => document.documentElement.classList.toggle("dark", media.matches);
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, []);
}

function reportFailure(error: unknown) {
  toast.error(t("action.failed", { message: String(error) }));
}

function describeRefusal(reason: string): string {
  return t(`refused.${reason}` as TextKey);
}

export default function App() {
  useSystemTheme();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [volumes, setVolumes] = useState<Volume[]>([]);
  const [letter, setLetter] = useState("");
  const [scan, setScan] = useState<ScanState>({ phase: "idle" });
  const [summary, setSummary] = useState<ScanSummary | null>(null);
  const [scanId, setScanId] = useState(0);
  const [generation, setGeneration] = useState(0);
  const [focus, setFocus] = useState(ROOT);
  const [selected, setSelected] = useState<Row | null>(null);
  const [hovered, setHovered] = useState<Row | null>(null);
  const [menuRow, setMenuRow] = useState<Row | null>(null);
  const [recycling, setRecycling] = useState<{
    row: Row;
    path: string;
    linked: boolean;
  } | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [tab, setTab] = useState("tree");
  const [extension, setExtension] = useState<string | null>(null);
  const autoScanned = useRef(false);

  const startScan = useCallback((target: string) => {
    setScan({ phase: "scanning", done: 0, total: 0 });
    api
      .startScan(target, (event) => {
        switch (event.kind) {
          case "progress":
            setScan({ phase: "scanning", done: event.done, total: event.total });
            break;
          case "finished":
            setSummary(event.summary);
            setScan({ phase: "done", summary: event.summary });
            setFocus(ROOT);
            setSelected(null);
            setHovered(null);
            setExtension(null);
            setScanId((current) => current + 1);
            break;
          case "cancelled":
            setScan((current) => (current.phase === "scanning" ? { phase: "idle" } : current));
            break;
          case "failed":
            setScan({ phase: "idle" });
            toast.error(t("scan.failed", { message: event.message }));
            break;
        }
      })
      .catch((error: unknown) => {
        setScan({ phase: "idle" });
        toast.error(t("scan.failed", { message: String(error) }));
      });
  }, []);

  useEffect(() => {
    void Promise.all([api.appInfo(), api.listVolumes()]).then(([appInfo, drives]) => {
      setInfo(appInfo);
      document.documentElement.classList.toggle("mica", appInfo.mica);
      setVolumes(drives);
      const preferred = appInfo.autoScan ?? "C";
      const initial = drives.find((drive) => drive.letter === preferred) ?? drives[0];
      if (initial) setLetter(initial.letter);
      if (appInfo.autoScan && initial && !autoScanned.current) {
        autoScanned.current = true;
        startScan(initial.letter);
      }
    });
  }, [startScan]);

  const select = useCallback((node: number) => {
    void api.getNode(node).then(setSelected).catch(reportFailure);
  }, []);

  const openNode = useCallback((node: number) => {
    setFocus(node);
    select(node);
  }, [select]);

  const showMenu = useCallback((node: number) => {
    void api.getNode(node).then((row) => {
      setMenuRow(row);
      setSelected(row);
    });
  }, []);

  const requestRecycle = useCallback((row: Row) => {
    if (row.refusal !== null) {
      toast.error(describeRefusal(row.refusal));
      return;
    }
    void Promise.all([api.getPath(row.id), api.countOtherLinks(row.id)])
      .then(([path, others]) => setRecycling({ row, path, linked: others > 0 }))
      .catch(reportFailure);
  }, []);

  const requestRecycleNode = useCallback(
    (node: number) => {
      void api.getNode(node).then(requestRecycle).catch(reportFailure);
    },
    [requestRecycle],
  );

  const confirmRecycle = useCallback(() => {
    const target = recycling;
    setRecycling(null);
    if (!target) return;
    void api
      .recycle([target.row.id])
      .then((result: RecycleResult) => {
        for (const refusal of result.refused) {
          toast.error(describeRefusal(refusal.reason));
        }
        if (result.aborted) toast.warning(t("recycle.aborted"));
        if (result.removed.length > 0) {
          toast.success(t("recycle.done", { size: formatBytes(result.freedBytes, locale) }));
          setSelected(null);
          setHovered(null);
          setFocus((current) => (result.removed.includes(current) ? ROOT : current));
          setGeneration((current) => current + 1);
        }
      })
      .catch(reportFailure);
  }, [recycling]);

  const copyPath = useCallback((row: Row) => {
    void api
      .copyPath(row.id)
      .then(() => toast.success(t("action.copied")))
      .catch(reportFailure);
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const typing = event.target instanceof HTMLInputElement;
      if (event.ctrlKey && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setSearchOpen(true);
      } else if (event.key === "Backspace" && !typing && focus !== ROOT) {
        event.preventDefault();
        void api.getAncestry(focus).then((rows) => {
          const parent = rows.at(-2);
          if (parent) setFocus(parent.id);
        });
      } else if (event.key === "F5") {
        event.preventDefault();
        if (letter && scan.phase !== "scanning") startScan(letter);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focus, letter, scan.phase, startScan]);

  const scanned = summary !== null && scanId > 0;
  const scanning = scan.phase === "scanning";
  const percent =
    scan.phase === "scanning" && scan.total > 0 ? (scan.done / scan.total) * 100 : undefined;

  return (
    <TooltipProvider delayDuration={300}>
      <main className="app-surface flex h-screen flex-col overflow-hidden text-foreground">
        <TitleBar
          volumes={volumes}
          letter={letter}
          scanning={scanning}
          scanned={scanned}
          // Until the app knows, it assumes rights, so the button does not flash.
          elevated={info?.elevated ?? true}
          onLetter={setLetter}
          onScan={() => startScan(letter)}
          onCancel={() => void api.cancelScan()}
          onSearch={() => setSearchOpen(true)}
          onReopen={() => void api.relaunchElevated(letter || null).catch(reportFailure)}
        />
        {scanning && (
          <div className="flex shrink-0 items-center gap-3 border-b px-3 py-1.5 text-xs text-muted-foreground">
            <span className="shrink-0">{t("scan.scanning")}</span>
            <Progress
              className="h-1"
              value={percent ?? null}
              aria-label={t("scan.scanning")}
            />
            <span className="shrink-0 tabular-nums">
              {scan.phase === "scanning" &&
                t("scan.counted", { count: formatCount(scan.done, locale) })}
            </span>
          </div>
        )}

        <ContextMenu>
          <div className="min-h-0 flex-1">
            {scanned ? (
              <ResizablePanelGroup orientation="horizontal">
                <ResizablePanel defaultSize="34%" minSize="22%">
                  <Tabs value={tab} onValueChange={setTab} className="flex h-full flex-col gap-0">
                    <TabsList className="mx-2 mt-2 shrink-0">
                      <TabsTrigger value="tree">{t("tabs.tree")}</TabsTrigger>
                      <TabsTrigger value="top">{t("tabs.top")}</TabsTrigger>
                      <TabsTrigger value="types">{t("tabs.types")}</TabsTrigger>
                    </TabsList>
                    <ContextMenuTrigger asChild>
                      <div className="mt-2 min-h-0 flex-1 border-t">
                        <TabsContent value="tree" className="h-full">
                          <TreePanel
                            scanId={scanId}
                            generation={generation}
                            selected={selected?.id ?? null}
                            onSelect={select}
                            onOpen={openNode}
                            onMenu={showMenu}
                            onRemove={requestRecycleNode}
                          />
                        </TabsContent>
                        <TabsContent value="top" className="h-full">
                          <TopFiles
                            scanId={scanId}
                            generation={generation}
                            extension={extension}
                            selected={selected?.id ?? null}
                            onClearFilter={() => setExtension(null)}
                            onSelect={select}
                            onMenu={showMenu}
                          />
                        </TabsContent>
                        <TabsContent value="types" className="h-full">
                          <FileTypes
                            scanId={scanId}
                            generation={generation}
                            onPick={(picked) => {
                              setExtension(picked);
                              setTab("top");
                            }}
                          />
                        </TabsContent>
                      </div>
                    </ContextMenuTrigger>
                  </Tabs>
                </ResizablePanel>
                <ResizableHandle />
                <ResizablePanel minSize="30%">
                  <div className="flex h-full flex-col">
                    <PathBar focus={focus} generation={generation} onOpen={openNode} />
                    <ContextMenuTrigger asChild>
                      <div className="min-h-0 flex-1">
                        <TreemapView
                          scanId={scanId}
                          generation={generation}
                          focus={focus}
                          selected={selected?.id ?? null}
                          onHover={setHovered}
                          onSelect={select}
                          onOpen={openNode}
                          onMenu={showMenu}
                        />
                      </div>
                    </ContextMenuTrigger>
                  </div>
                </ResizablePanel>
              </ResizablePanelGroup>
            ) : (
              <VolumePicker
                volumes={volumes}
                onPick={(picked) => {
                  setLetter(picked);
                  if (!scanning) startScan(picked);
                }}
              />
            )}
          </div>
          <NodeMenu
            row={menuRow}
            onOpen={(row) => {
              if (isDirectory(row)) openNode(row.id);
            }}
            onReveal={(row) => void api.revealInExplorer(row.id).catch(reportFailure)}
            onCopyPath={copyPath}
            onProperties={(row) => void api.showProperties(row.id).catch(reportFailure)}
            onRecycle={requestRecycle}
          />
        </ContextMenu>

        <StatusBar
          shown={hovered ?? selected}
          summary={summary}
        />
      </main>

      <SearchDialog
        open={searchOpen}
        scanned={scanned}
        onOpenChange={setSearchOpen}
        onPick={(node) => {
          select(node);
          setTab("tree");
        }}
      />
      <ConfirmRecycle
        row={recycling?.row ?? null}
        path={recycling?.path ?? ""}
        linked={recycling?.linked ?? false}
        onCancel={() => setRecycling(null)}
        onConfirm={confirmRecycle}
      />
      <Toaster position="bottom-right" theme="system" />
    </TooltipProvider>
  );
}
