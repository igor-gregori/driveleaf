// M0 interface spike: measures IPC transfer, GPU upload, hit testing and zoom FPS.
// Goes away with the rest of the spike when the real commands land in M2.

import { invoke } from "@tauri-apps/api/core";
import type { TreemapRenderer } from "@/treemap/renderer";
import { TileBuffer } from "@/treemap/tiles";

const COUNTS = [50_000, 100_000, 200_000];
const TRANSFER_RUNS = 15;
const HIT_TESTS = 2_000;
const ZOOM_MS = 5_000;
const THROUGHPUT_FRAMES = 60;

export interface Prepared {
  tiles: number;
  bytes: number;
  generate_ms: number;
}

/** Runs one prepare and fetch cycle and shows the result. */
export async function loadTiles(
  renderer: TreemapRenderer,
  count: number,
  width: number,
  height: number,
): Promise<TileBuffer> {
  await invoke<Prepared>("spike_prepare", { count, width, height, seed: 42 });
  const bytes = await invoke<ArrayBuffer>("spike_fetch");
  renderer.upload(bytes);
  return new TileBuffer(bytes);
}

function summarize(samples: number[]) {
  const sorted = [...samples].sort((a, b) => a - b);
  const at = (quantile: number) =>
    sorted[Math.min(sorted.length - 1, Math.floor(quantile * sorted.length))] ?? 0;
  const total = samples.reduce((sum, value) => sum + value, 0);
  return {
    n: samples.length,
    mean: total / Math.max(1, samples.length),
    median: at(0.5),
    p95: at(0.95),
    p99: at(0.99),
    max: sorted.at(-1) ?? 0,
  };
}

function nextFrame(): Promise<number> {
  return new Promise((resolve) => requestAnimationFrame(resolve));
}

/** Zooms in and out around the center for a few seconds, recording frame times. */
async function zoomRun(renderer: TreemapRenderer, width: number, height: number) {
  const frameMs: number[] = [];
  const drawMs: number[] = [];
  const started = await nextFrame();
  let last = started;
  while (last - started < ZOOM_MS) {
    const now = await Promise.race([
      nextFrame(),
      new Promise<null>((resolve) => setTimeout(() => resolve(null), 1_000)),
    ]);
    if (now === null) {
      return { stalled: true, frameMs: summarize(frameMs), drawMs: summarize(drawMs), fps: 0 };
    }
    frameMs.push(now - last);
    last = now;
    const phase = (now - started) / ZOOM_MS;
    const scale = 1 + 7 * (0.5 - 0.5 * Math.cos(4 * Math.PI * phase));
    renderer.view.scale = scale;
    renderer.view.x = (width / 2) * (1 - scale);
    renderer.view.y = (height / 2) * (1 - scale);
    const before = performance.now();
    renderer.draw();
    drawMs.push(performance.now() - before);
  }
  renderer.view.scale = 1;
  renderer.view.x = 0;
  renderer.view.y = 0;
  const totalMs = frameMs.reduce((sum, value) => sum + value, 0);
  return {
    stalled: false,
    frameMs: summarize(frameMs),
    drawMs: summarize(drawMs),
    fps: (frameMs.length * 1000) / totalMs,
  };
}

/** Times complete frames with no display sync, so it works on a hidden window too. */
function gpuThroughput(renderer: TreemapRenderer) {
  const samples: number[] = [];
  for (let frame = 0; frame < THROUGHPUT_FRAMES; frame++) {
    const before = performance.now();
    renderer.draw();
    renderer.waitForGpu();
    samples.push(performance.now() - before);
  }
  return summarize(samples);
}

function hitTestRun(tiles: TileBuffer, width: number, height: number) {
  const samples: number[] = [];
  let found = 0;
  for (let test = 0; test < HIT_TESTS; test++) {
    const x = Math.random() * width;
    const y = Math.random() * height;
    const before = performance.now();
    if (tiles.nodeAt(x, y) !== null) found++;
    samples.push(performance.now() - before);
  }
  return { ...summarize(samples), found };
}

async function measure(renderer: TreemapRenderer, count: number, width: number, height: number) {
  const prepared = await invoke<Prepared>("spike_prepare", { count, width, height, seed: 42 });

  const transferMs: number[] = [];
  let bytes = new ArrayBuffer(0);
  for (let run = 0; run < TRANSFER_RUNS; run++) {
    const before = performance.now();
    bytes = await invoke<ArrayBuffer>("spike_fetch");
    transferMs.push(performance.now() - before);
  }

  const beforeUpload = performance.now();
  renderer.upload(bytes);
  renderer.waitForGpu();
  const uploadMs = performance.now() - beforeUpload;

  const tiles = new TileBuffer(bytes);
  renderer.draw();
  return {
    count,
    bytes: prepared.bytes,
    generateMs: prepared.generate_ms,
    transferMs: summarize(transferMs),
    uploadMs,
    hitTestMs: hitTestRun(tiles, width, height),
    gpuFrameMs: gpuThroughput(renderer),
    zoom: await zoomRun(renderer, width, height),
  };
}

/** Runs the whole benchmark, reporting progress through `onStatus`. */
export async function runBenchmark(
  renderer: TreemapRenderer,
  width: number,
  height: number,
  onStatus: (status: string) => void,
) {
  const runs = [];
  for (const count of COUNTS) {
    onStatus(`measuring ${count.toLocaleString("en-US")} tiles`);
    runs.push(await measure(renderer, count, width, height));
  }
  const memory = (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory;
  return {
    gpu: renderer.gpuName(),
    canvas: { width, height, pixelRatio: window.devicePixelRatio },
    visibility: document.visibilityState,
    jsHeapMb: memory ? memory.usedJSHeapSize / 1_048_576 : null,
    userAgent: navigator.userAgent,
    runs,
  };
}
