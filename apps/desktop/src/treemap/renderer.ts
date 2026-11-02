import { parseHexColor } from "./color";
import { TILE_BYTES, type TileRect } from "./tiles";

/** Space each file tile gives up on every side, in CSS pixels. */
const TILE_GAP = 1.25;

const VERTEX_SHADER = `#version 300 es
layout(location = 0) in vec2 a_corner;
layout(location = 1) in vec4 a_rect;
layout(location = 2) in uint a_style;
uniform vec2 u_resolution;
uniform vec3 u_view;
uniform int u_hover;
uniform int u_selected;
uniform vec3 u_palette[10];
uniform vec3 u_highlight;
uniform float u_folder_step;
uniform float u_gap;
out vec2 v_local;
out vec2 v_size;
out vec3 v_color;
flat out float v_folder;
flat out float v_outline;

void main() {
  uint category = a_style & 255u;
  float depth = float((a_style >> 8) & 255u);
  bool folder = (a_style & 65536u) != 0u;

  // Each tile shrinks a little so neighbours stand apart. Folders shrink less,
  // which keeps a margin around their contents.
  vec2 size = a_rect.zw * u_view.z;
  float gap = min((folder ? 0.5 : 1.0) * u_gap, 0.2 * min(size.x, size.y));
  vec2 origin = a_rect.xy * u_view.z + u_view.xy + gap;
  size -= 2.0 * gap;
  vec2 position = origin + a_corner * size;
  v_local = a_corner * size;
  v_size = size;

  vec3 color = u_palette[category];
  if (folder) {
    color = color * (1.0 + u_folder_step * min(depth, 8.0));
  }
  float hovered = gl_InstanceID == u_hover ? 1.0 : 0.0;
  v_color = mix(color, u_highlight, hovered * 0.22);
  v_folder = folder ? 1.0 : 0.0;
  v_outline = gl_InstanceID == u_selected ? 1.0 : 0.0;

  vec2 clip = position / u_resolution * 2.0 - 1.0;
  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
}`;

const FRAGMENT_SHADER = `#version 300 es
precision highp float;
uniform vec3 u_highlight;
in vec2 v_local;
in vec2 v_size;
in vec3 v_color;
flat in float v_folder;
flat in float v_outline;
out vec4 fragment;

void main() {
  float radius = min(4.5, 0.3 * min(v_size.x, v_size.y));
  vec2 q = abs(v_local - 0.5 * v_size) - (0.5 * v_size - radius);
  float distance = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - radius;
  float coverage = clamp(0.5 - distance, 0.0, 1.0);

  // A soft light from the top left gives flat tiles some volume.
  vec2 unit = v_local / max(v_size, vec2(1.0));
  float shade = 1.0 + (0.5 - unit.y) * 0.16 + (0.5 - unit.x) * 0.08;
  vec3 color = v_color * mix(shade, 1.0, v_folder);

  if (v_outline > 0.5) {
    float edge = clamp(distance + 2.5, 0.0, 1.0);
    color = mix(color, u_highlight, edge);
  }
  fragment = vec4(color * coverage, coverage);
}`;

export interface View {
  x: number;
  y: number;
  scale: number;
}

export interface Palette {
  /** Ten RGB colors in the order of the category numbers, each from 0 to 1. */
  colors: [number, number, number][];
  background: [number, number, number];
  highlight: [number, number, number];
  /** How much each level of nesting changes a folder's brightness. */
  folderStep: number;
}

function compile(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type);
  if (!shader) throw new Error("could not create shader");
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    throw new Error(gl.getShaderInfoLog(shader) ?? "shader did not compile");
  }
  return shader;
}

/** Draws tiles with instancing; the layout is uploaded once and only the view changes. */
export class TreemapRenderer {
  readonly view: View = { x: 0, y: 0, scale: 1 };
  hover = -1;
  selected = -1;
  private readonly gl: WebGL2RenderingContext;
  private readonly instances: WebGLBuffer;
  private readonly uniforms: Record<string, WebGLUniformLocation | null>;
  private background: [number, number, number] = [0, 0, 0];
  private tileCount = 0;
  private cssWidth = 1;
  private cssHeight = 1;

  constructor(private readonly canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", {
      alpha: false,
      antialias: false,
      powerPreference: "high-performance",
    });
    if (!gl) throw new Error("WebGL2 is not available");
    this.gl = gl;

    const program = gl.createProgram();
    gl.attachShader(program, compile(gl, gl.VERTEX_SHADER, VERTEX_SHADER));
    gl.attachShader(program, compile(gl, gl.FRAGMENT_SHADER, FRAGMENT_SHADER));
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      throw new Error(gl.getProgramInfoLog(program) ?? "program did not link");
    }
    gl.useProgram(program);
    const uniform = (name: string) => gl.getUniformLocation(program, name);
    this.uniforms = {
      resolution: uniform("u_resolution"),
      view: uniform("u_view"),
      hover: uniform("u_hover"),
      selected: uniform("u_selected"),
      palette: uniform("u_palette"),
      highlight: uniform("u_highlight"),
      folderStep: uniform("u_folder_step"),
    };
    gl.uniform1f(uniform("u_gap"), TILE_GAP);

    gl.bindVertexArray(gl.createVertexArray());

    gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

    this.instances = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.instances);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 4, gl.FLOAT, false, TILE_BYTES, 0);
    gl.vertexAttribDivisor(1, 1);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribIPointer(2, 1, gl.UNSIGNED_INT, TILE_BYTES, 16);
    gl.vertexAttribDivisor(2, 1);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
  }

  /** Describes the GPU, to spot software rendering. */
  gpuName(): string {
    const gl = this.gl;
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    return info ? String(gl.getParameter(info.UNMASKED_RENDERER_WEBGL)) : "unknown";
  }

  setPalette(palette: Palette): void {
    const gl = this.gl;
    gl.uniform3fv(this.uniforms.palette, palette.colors.flat());
    gl.uniform3f(this.uniforms.highlight, ...palette.highlight);
    gl.uniform1f(this.uniforms.folderStep, palette.folderStep);
    this.background = palette.background;
  }

  resize(cssWidth: number, cssHeight: number, pixelRatio: number): void {
    this.cssWidth = Math.max(1, cssWidth);
    this.cssHeight = Math.max(1, cssHeight);
    this.canvas.width = Math.round(this.cssWidth * pixelRatio);
    this.canvas.height = Math.round(this.cssHeight * pixelRatio);
    this.gl.viewport(0, 0, this.canvas.width, this.canvas.height);
  }

  upload(bytes: ArrayBuffer): void {
    const gl = this.gl;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.instances);
    gl.bufferData(gl.ARRAY_BUFFER, bytes, gl.STATIC_DRAW);
    this.tileCount = bytes.byteLength / TILE_BYTES;
    this.hover = -1;
    this.selected = -1;
  }

  draw(): void {
    const gl = this.gl;
    gl.clearColor(...this.background, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.uniform2f(this.uniforms.resolution, this.cssWidth, this.cssHeight);
    gl.uniform3f(this.uniforms.view, this.view.x, this.view.y, this.view.scale);
    gl.uniform1i(this.uniforms.hover, this.hover);
    gl.uniform1i(this.uniforms.selected, this.selected);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.tileCount);
  }

  /** The view that makes a layout rectangle fill the canvas. */
  viewOnto(rect: TileRect): View {
    const scale = Math.min(this.cssWidth / rect.w, this.cssHeight / rect.h);
    return {
      scale,
      x: (this.cssWidth - rect.w * scale) / 2 - rect.x * scale,
      y: (this.cssHeight - rect.h * scale) / 2 - rect.y * scale,
    };
  }

  setView(view: View): void {
    Object.assign(this.view, view);
  }

  /** Converts a point on the canvas to layout space. */
  toLayout(cssX: number, cssY: number): [number, number] {
    const { x, y, scale } = this.view;
    return [(cssX - x) / scale, (cssY - y) / scale];
  }
}

/** Reads the theme's colors from CSS variables, so the treemap follows the theme. */
export function readPalette(element: HTMLElement): Palette {
  const style = getComputedStyle(element);
  // A 2D context turns any CSS color into plain #rrggbb.
  const probe = document.createElement("canvas").getContext("2d");
  const color = (name: string): [number, number, number] => {
    const text = style.getPropertyValue(name).trim();
    if (!probe) return parseHexColor(text);
    probe.fillStyle = text;
    return parseHexColor(String(probe.fillStyle));
  };
  const names = [
    "folder",
    "other",
    "document",
    "image",
    "video",
    "audio",
    "archive",
    "code",
    "application",
    "system",
  ];
  return {
    colors: names.map((name) => color(`--treemap-${name}`)),
    background: color("--treemap-background"),
    highlight: color("--treemap-highlight"),
    folderStep: Number.parseFloat(style.getPropertyValue("--treemap-folder-step")),
  };
}
