import { useCallback, useEffect, useRef, useState } from "react";
import { locale, t } from "@/i18n";
import { type Row, api } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { type Label, drawLabels, labelCandidates } from "@/treemap/labels";
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
  const laidOutSizeRef = useRef("");
  const labelCanvasRef = useRef<HTMLCanvasElement>(null);
  const labelsRef = useRef<Label[]>([]);
  const labelTicketRef = useRef(0);
  const paletteRef = useRef<Palette | null>(null);
  const [unavailable, setUnavailable] = useState(false);

  const paintLabels = useCallback(() => {
    const canvas = labelCanvasRef.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;
    context.setTransform(1, 0, 0, 1, 0, 0);
    context.clearRect(0, 0, canvas.width, canvas.height);
    const tiles = tilesRef.current;
    const palette = paletteRef.current;
    if (tiles && palette) {
      drawLabels(context, tiles, labelsRef.current, palette, window.devicePixelRatio);
    }
  }, []);

  const clearLabels = useCallback(() => {
    labelTicketRef.current++;
    labelsRef.current = [];
    paintLabels();
  }, [paintLabels]);

  // Labels are drawn in layout space, so they only show while the view is at rest.
  const refreshLabels = useCallback(async () => {
    const tiles = tilesRef.current;
    if (!tiles) return;
    const ticket = ++labelTicketRef.current;
    const indices = labelCandidates(tiles);
    if (indices.length === 0) return;
    let rows: Row[];
    try {
      rows = await api.getRows(indices.map((index) => tiles.nodeOf(index)));
    } catch {
      // A new scan replaced the tree meanwhile; its own load brings new labels.
      return;
    }
    if (ticket !== labelTicketRef.current || tilesRef.current !== tiles) return;
    labelsRef.current = indices.map((index, position) => ({
      index,
      name: rows[position].name,
      detail: formatBytes(rows[position].allocated, locale),
    }));
    paintLabels();
  }, [paintLabels]);

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
    laidOutSizeRef.current = `${width}x${height}`;
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
      clearLabels();
      renderer.upload(bytes);
      renderer.setView(identity);
      applySelection();
      renderer.draw();
    };

    const inward = current && sameScan && previous !== focus ? current.indexOfNode(focus) : -1;
    const outward = sameScan && previous !== null && previous !== focus ? next.indexOfNode(previous) : -1;
    if (inward >= 0) {
      clearLabels();
      animate(identity, renderer.viewOnto(current!.rectOf(inward)), () => {
        show();
        void refreshLabels();
      });
    } else if (outward >= 0) {
      const from = renderer.viewOnto(next.rectOf(outward));
      show();
      renderer.setView(from);
      animate(from, identity, () => {
        renderer.setView(identity);
        void refreshLabels();
      });
    } else {
      show();
      void refreshLabels();
    }
  }, [animate, applySelection, clearLabels, focus, refreshLabels, scanId]);

  const latestLoadRef = useRef(load);
  latestLoadRef.current = load;

  // The renderer lives as long as the view. Changing folders only loads tiles.
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
      paletteRef.current = palette;
      renderer.setPalette(palette);
      renderer.draw();
      paintLabels();
    };
    applyTheme(readPalette(document.documentElement));
    const themeWatcher = new MutationObserver(() => applyTheme(readPalette(document.documentElement)));
    themeWatcher.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });

    const fit = () => {
      const { width, height } = canvas.getBoundingClientRect();
      renderer.resize(width, height, window.devicePixelRatio);
      const overlay = labelCanvasRef.current;
      if (overlay) {
        overlay.width = Math.round(Math.max(1, width) * window.devicePixelRatio);
        overlay.height = Math.round(Math.max(1, height) * window.devicePixelRatio);
        paintLabels();
      }
      return `${width}x${height}`;
    };
    fit();

    let pending = 0;
    const observer = new ResizeObserver(() => {
      const size = fit();
      renderer.draw();
      window.clearTimeout(pending);
      if (size !== laidOutSizeRef.current) {
        pending = window.setTimeout(() => void latestLoadRef.current(), RELAYOUT_DELAY_MS);
      }
    });
    observer.observe(canvas);
    return () => {
      window.clearTimeout(pending);
      observer.disconnect();
      themeWatcher.disconnect();
    };
  }, [paintLabels]);

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
    event.currentTarget.style.cursor = index >= 0 ? "pointer" : "default";
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
    <div className="relative size-full">
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
      <canvas ref={labelCanvasRef} className="pointer-events-none absolute inset-0 size-full" />
    </div>
  );
}
