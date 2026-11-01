import { useCallback, useEffect, useRef, useState } from "react";
import { t } from "@/i18n";
import { type Row, api } from "@/lib/api";
import { type Palette, TreemapRenderer, type View, readPalette } from "@/treemap/renderer";
import { TileBuffer } from "@/treemap/tiles";

const ANIMATION_MS = 220;
const RELAYOUT_DELAY_MS = 120;
const MAX_TILES = 200_000;
/** Software rendering cannot keep up with the full detail. */
const MAX_TILES_SOFTWARE = 40_000;

function isSoftwareRenderer(name: string): boolean {
  return /swiftshader|basic render|llvmpipe|software/i.test(name);
}

function easeOut(progress: number): number {
  return 1 - (1 - progress) ** 3;
}

interface Props {
  /** Changes with every new scan, which invalidates all node IDs. */
  scanId: number;
  /** Changes when the tree does, such as after a removal. */
  generation: number;
  focus: number;
  selected: number | null;
  onHover: (row: Row | null) => void;
  onSelect: (node: number) => void;
  onOpen: (node: number) => void;
  onMenu: (node: number) => void;
}

export function TreemapView({
  scanId,
  generation,
  focus,
  selected,
  onHover,
  onSelect,
  onOpen,
  onMenu,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<TreemapRenderer | null>(null);
  const tilesRef = useRef<TileBuffer | null>(null);
  const shownFocusRef = useRef<number | null>(null);
  const shownScanRef = useRef<number | null>(null);
  const softwareRef = useRef(false);
  const frameRef = useRef(0);
  const hoverRef = useRef<number | null>(null);
  const loadRef = useRef(0);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const [unavailable, setUnavailable] = useState(false);

  const redraw = useCallback(() => {
    if (frameRef.current) return;
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = 0;
      rendererRef.current?.draw();
    });
  }, []);

  const applySelection = useCallback(() => {
    const renderer = rendererRef.current;
    const tiles = tilesRef.current;
    if (!renderer || !tiles) return;
    const node = selectedRef.current;
    renderer.selected = node === null ? -1 : tiles.indexOfNode(node);
  }, []);

  const animate = useCallback(
    (from: View, to: View, done: () => void) => {
      const renderer = rendererRef.current;
      if (!renderer || softwareRef.current) {
        done();
        return;
      }
      const started = performance.now();
      const step = (now: number) => {
        const progress = Math.min(1, (now - started) / ANIMATION_MS);
        const eased = easeOut(progress);
        renderer.setView({
          x: from.x + (to.x - from.x) * eased,
          y: from.y + (to.y - from.y) * eased,
          scale: from.scale + (to.scale - from.scale) * eased,
        });
        renderer.draw();
        if (progress < 1) {
          requestAnimationFrame(step);
        } else {
          done();
        }
      };
      requestAnimationFrame(step);
    },
    [],
  );

  const load = useCallback(async () => {
    const renderer = rendererRef.current;
    const canvas = canvasRef.current;
    if (!renderer || !canvas) return;
    const ticket = ++loadRef.current;
    const { width, height } = canvas.getBoundingClientRect();
    const limit = softwareRef.current ? MAX_TILES_SOFTWARE : MAX_TILES;
    const bytes = await api.getTreemap(focus, width, height, limit);
    if (ticket !== loadRef.current) return;

    const next = new TileBuffer(bytes);
    const current = tilesRef.current;
    const previous = shownFocusRef.current;
    const sameScan = shownScanRef.current === scanId;
    const identity: View = { x: 0, y: 0, scale: 1 };
    const show = () => {
      tilesRef.current = next;
      shownFocusRef.current = focus;
      shownScanRef.current = scanId;
      renderer.upload(bytes);
      renderer.setView(identity);
      applySelection();
      renderer.draw();
    };

    const inward = current && sameScan && previous !== focus ? current.indexOfNode(focus) : -1;
    const outward = sameScan && previous !== null && previous !== focus ? next.indexOfNode(previous) : -1;
    if (inward >= 0) {
      animate(identity, renderer.viewOnto(current!.rectOf(inward)), show);
    } else if (outward >= 0) {
      const from = renderer.viewOnto(next.rectOf(outward));
      show();
      renderer.setView(from);
      animate(from, identity, () => renderer.setView(identity));
    } else {
      show();
    }
  }, [animate, applySelection, focus, scanId]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let renderer: TreemapRenderer;
    try {
      renderer = rendererRef.current ??= new TreemapRenderer(canvas);
    } catch {
      // Without WebGL2 the tree and the lists still work, so only this view gives up.
      setUnavailable(true);
      return;
    }
    softwareRef.current = isSoftwareRenderer(renderer.gpuName());

    const applyTheme = (palette: Palette) => {
      renderer.setPalette(palette);
      renderer.draw();
    };
    applyTheme(readPalette(document.documentElement));
    const themeWatcher = new MutationObserver(() => applyTheme(readPalette(document.documentElement)));
    themeWatcher.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });

    let pending = 0;
    const observer = new ResizeObserver(() => {
      const { width, height } = canvas.getBoundingClientRect();
      renderer.resize(width, height, window.devicePixelRatio);
      renderer.draw();
      window.clearTimeout(pending);
      pending = window.setTimeout(() => void load(), RELAYOUT_DELAY_MS);
    });
    observer.observe(canvas);
    return () => {
      window.clearTimeout(pending);
      observer.disconnect();
      themeWatcher.disconnect();
    };
  }, [load]);

  useEffect(() => {
    void load();
  }, [load, generation]);

  useEffect(() => {
    applySelection();
    redraw();
  }, [applySelection, redraw, selected]);

  const tileAt = (event: React.MouseEvent<HTMLCanvasElement>) => {
    const renderer = rendererRef.current;
    const tiles = tilesRef.current;
    if (!renderer || !tiles) return -1;
    const bounds = event.currentTarget.getBoundingClientRect();
    const [x, y] = renderer.toLayout(event.clientX - bounds.left, event.clientY - bounds.top);
    return tiles.indexAt(x, y);
  };

  const onMouseMove = (event: React.MouseEvent<HTMLCanvasElement>) => {
    const renderer = rendererRef.current;
    const tiles = tilesRef.current;
    if (!renderer || !tiles) return;
    const index = tileAt(event);
    if (index === renderer.hover) return;
    renderer.hover = index;
    redraw();

    // The highlight above never waits for IPC; only the status text does.
    const node = index < 0 ? null : tiles.nodeOf(index);
    hoverRef.current = node;
    if (node === null) {
      onHover(null);
      return;
    }
    void api.getNode(node).then((row) => {
      if (hoverRef.current === node) onHover(row);
    });
  };

  const onMouseLeave = () => {
    const renderer = rendererRef.current;
    if (renderer) {
      renderer.hover = -1;
      redraw();
    }
    hoverRef.current = null;
    onHover(null);
  };

  const nodeUnder = (event: React.MouseEvent<HTMLCanvasElement>) => {
    const tiles = tilesRef.current;
    const index = tileAt(event);
    return !tiles || index < 0 ? null : tiles.nodeOf(index);
  };

  if (unavailable) {
    return (
      <div className="grid size-full place-items-center p-8 text-center text-sm text-muted-foreground">
        {t("treemap.unavailable")}
      </div>
    );
  }

  return (
    <canvas
      ref={canvasRef}
      className="size-full"
      onMouseMove={onMouseMove}
      onMouseLeave={onMouseLeave}
      onClick={(event) => {
        const node = nodeUnder(event);
        if (node !== null) onSelect(node);
      }}
      onDoubleClick={(event) => {
        const tiles = tilesRef.current;
        const index = tileAt(event);
        if (tiles && index >= 0 && tiles.isFolder(index)) onOpen(tiles.nodeOf(index));
      }}
      onContextMenu={(event) => {
        const node = nodeUnder(event);
        if (node !== null) onMenu(node);
      }}
    />
  );
}
