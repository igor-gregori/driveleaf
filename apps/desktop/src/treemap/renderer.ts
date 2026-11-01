import { TILE_BYTES } from "./tiles";

const VERTEX_SHADER = `#version 300 es
layout(location = 0) in vec2 a_corner;
layout(location = 1) in vec4 a_rect;
layout(location = 2) in vec4 a_color;
uniform vec2 u_resolution;
uniform vec3 u_view;
out vec2 v_local;
out vec2 v_size;
out vec3 v_color;
void main() {
  vec2 size = a_rect.zw * u_view.z;
  vec2 position = a_rect.xy * u_view.z + u_view.xy + a_corner * size;
  v_local = a_corner * size;
  v_size = size;
  v_color = a_color.abg;
  vec2 clip = position / u_resolution * 2.0 - 1.0;
  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
}`;

const FRAGMENT_SHADER = `#version 300 es
precision mediump float;
in vec2 v_local;
in vec2 v_size;
in vec3 v_color;
out vec4 fragment;
void main() {
  float radius = min(2.5, 0.3 * min(v_size.x, v_size.y));
  vec2 q = abs(v_local - 0.5 * v_size) - (0.5 * v_size - radius);
  float distance = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - radius;
  float coverage = clamp(0.5 - distance, 0.0, 1.0);
  fragment = vec4(v_color * coverage, coverage);
}`;

export interface View {
  x: number;
  y: number;
  scale: number;
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
  private readonly gl: WebGL2RenderingContext;
  private readonly instances: WebGLBuffer;
  private readonly resolutionUniform: WebGLUniformLocation | null;
  private readonly viewUniform: WebGLUniformLocation | null;
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
    this.resolutionUniform = gl.getUniformLocation(program, "u_resolution");
    this.viewUniform = gl.getUniformLocation(program, "u_view");

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
    gl.vertexAttribPointer(2, 4, gl.UNSIGNED_BYTE, true, TILE_BYTES, 16);
    gl.vertexAttribDivisor(2, 1);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
  }

  /** Describes the GPU, for benchmark reports. */
  gpuName(): string {
    const gl = this.gl;
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    return info ? String(gl.getParameter(info.UNMASKED_RENDERER_WEBGL)) : "unknown";
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
  }

  draw(): void {
    const gl = this.gl;
    gl.clearColor(0.09, 0.09, 0.1, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.uniform2f(this.resolutionUniform, this.cssWidth, this.cssHeight);
    gl.uniform3f(this.viewUniform, this.view.x, this.view.y, this.view.scale);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.tileCount);
  }

  /** Blocks until the GPU has finished everything queued so far. */
  waitForGpu(): void {
    const pixel = new Uint8Array(4);
    this.gl.readPixels(0, 0, 1, 1, this.gl.RGBA, this.gl.UNSIGNED_BYTE, pixel);
  }

  /** Converts a point on the canvas to layout space. */
  toLayout(cssX: number, cssY: number): [number, number] {
    const { x, y, scale } = this.view;
    return [(cssX - x) / scale, (cssY - y) / scale];
  }
}
