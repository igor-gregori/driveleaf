import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Gauge } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { loadTiles, runBenchmark } from "@/spike/bench";
import { TreemapRenderer } from "@/treemap/renderer";
import type { TileBuffer } from "@/treemap/tiles";

const TILE_COUNTS = [50_000, 100_000, 200_000];

function useSystemTheme() {
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => document.documentElement.classList.toggle("dark", media.matches);
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, []);
}

export default function App() {
  useSystemTheme();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<TreemapRenderer | null>(null);
  const tilesRef = useRef<TileBuffer | null>(null);
  const benchStartedRef = useRef(false);
  const [status, setStatus] = useState("idle");
  const [hovered, setHovered] = useState<number | null>(null);

  const load = useCallback(async (count: number) => {
    const renderer = rendererRef.current;
    const canvas = canvasRef.current;
    if (!renderer || !canvas) return;
    setStatus(`loading ${count.toLocaleString("en-US")} tiles`);
    const { width, height } = canvas.getBoundingClientRect();
    tilesRef.current = await loadTiles(renderer, count, width, height);
    renderer.draw();
    setStatus(`${count.toLocaleString("en-US")} tiles`);
  }, []);

  const benchmark = useCallback(async () => {
    const renderer = rendererRef.current;
    const canvas = canvasRef.current;
    if (!renderer || !canvas) return;
    const { width, height } = canvas.getBoundingClientRect();
    const report = await runBenchmark(renderer, width, height, setStatus);
    await invoke("spike_report", { json: JSON.stringify(report, null, 2) });
    const last = report.runs.at(-1);
    setStatus(`done: ${last?.zoom.fps.toFixed(1)} FPS at ${last?.count.toLocaleString("en-US")}`);
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const renderer = (rendererRef.current ??= new TreemapRenderer(canvas));
    const observer = new ResizeObserver(() => {
      const { width, height } = canvas.getBoundingClientRect();
      renderer.resize(width, height, window.devicePixelRatio);
      renderer.draw();
    });
    observer.observe(canvas);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (benchStartedRef.current) return;
    benchStartedRef.current = true;
    void invoke<boolean>("spike_mode").then((unattended) => {
      if (unattended) void benchmark();
    });
  }, [benchmark]);

  const onMouseMove = (event: React.MouseEvent<HTMLCanvasElement>) => {
    const renderer = rendererRef.current;
    const tiles = tilesRef.current;
    if (!renderer || !tiles) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const [x, y] = renderer.toLayout(event.clientX - bounds.left, event.clientY - bounds.top);
    setHovered(tiles.nodeAt(x, y));
  };

  const onWheel = (event: React.WheelEvent<HTMLCanvasElement>) => {
    const renderer = rendererRef.current;
    if (!renderer) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const cursorX = event.clientX - bounds.left;
    const cursorY = event.clientY - bounds.top;
    const { view } = renderer;
    const scale = Math.min(64, Math.max(1, view.scale * Math.exp(-event.deltaY * 0.002)));
    view.x = cursorX - ((cursorX - view.x) / view.scale) * scale;
    view.y = cursorY - ((cursorY - view.y) / view.scale) * scale;
    view.scale = scale;
    renderer.draw();
  };

  return (
    <main className="flex h-screen flex-col overflow-hidden bg-background text-foreground">
      <header className="flex items-center gap-2 border-b px-4 py-2">
        <h1 className="mr-4 text-sm font-semibold">Driveleaf</h1>
        {TILE_COUNTS.map((count) => (
          <Button key={count} variant="outline" size="sm" onClick={() => void load(count)}>
            {count / 1000}k
          </Button>
        ))}
        <Button size="sm" onClick={() => void benchmark()}>
          <Gauge />
          Benchmark
        </Button>
        <Badge variant="secondary" className="ml-auto">
          {status}
        </Badge>
        <Badge variant="outline">{hovered === null ? "no tile" : `tile ${hovered}`}</Badge>
      </header>
      <canvas
        ref={canvasRef}
        className="min-h-0 w-full flex-1"
        onMouseMove={onMouseMove}
        onWheel={onWheel}
      />
    </main>
  );
}
