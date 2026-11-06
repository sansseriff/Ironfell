/**
 * Extra pointer trackers for the Latency view (plans/rendering/latency-view.md):
 * a Konva ring, and two control rings drawn by the browser's GPU canvases
 * outside Iron (WebGPU, WebGL2). The GPU controls separate what a GPU canvas
 * costs in this browser from what Iron's engine adds: each draws one ring on the
 * main thread in a requestAnimationFrame callback, the same moment Iron's
 * main-thread runtime renders.
 *
 * Each tracker: `move(x, y)` in the pointer event (CSS px in its stage, NaN
 * when outside), `resize(w, h, dpr)`, `destroy()`.
 */
import Konva from 'konva';

export interface Tracker {
  move(x: number, y: number): void;
  resize(width: number, height: number, dpr: number): void;
  destroy(): void;
}

function rgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

/** Konva as a Konva app tracks a pointer: a non-listening layer, the shape
 *  moved in the event and `batchDraw()` (Konva's rAF-batched redraw). */
export function konvaTracker(container: HTMLDivElement, color: string, radius: number, stroke: number): Tracker {
  const stage = new Konva.Stage({ container, width: 1, height: 1, listening: false });
  const layer = new Konva.Layer({ listening: false });
  const circle = new Konva.Circle({ radius, stroke: color, strokeWidth: stroke, listening: false, perfectDrawEnabled: false, visible: false });
  layer.add(circle);
  stage.add(layer);
  return {
    move(x, y) {
      const inside = !Number.isNaN(x);
      circle.visible(inside);
      if (inside) circle.position({ x, y });
      layer.batchDraw();
    },
    resize(width, height) {
      stage.size({ width, height });
      layer.batchDraw();
    },
    destroy() {
      stage.destroy();
    },
  };
}

/** Shared by both GPU controls: a full-canvas triangle whose fragment shader
 *  keeps the pixels within `stroke / 2` of the ring. */
type RingState = { x: number; y: number; dirty: boolean; dpr: number };

function rafLoop(state: RingState, draw: () => void): () => void {
  let id = 0;
  const frame = () => {
    if (state.dirty) {
      state.dirty = false;
      draw();
    }
    id = requestAnimationFrame(frame);
  };
  id = requestAnimationFrame(frame);
  return () => cancelAnimationFrame(id);
}

/** A WebGPU canvas drawing one ring on rAF; null where WebGPU is missing. */
export async function webgpuTracker(canvas: HTMLCanvasElement, color: string, radius: number, stroke: number): Promise<Tracker | null> {
  const gpu = (navigator as Navigator & { gpu?: any }).gpu;
  const adapter = await gpu?.requestAdapter?.();
  const device = await adapter?.requestDevice?.();
  if (!device) return null;
  const context = canvas.getContext('webgpu') as any;
  if (!context) return null;
  const format = gpu.getPreferredCanvasFormat();
  context.configure({ device, format, alphaMode: 'premultiplied' });
  const module = device.createShaderModule({
    code: `
      struct U { centre: vec2f, radius: f32, stroke: f32, color: vec4f };
      @group(0) @binding(0) var<uniform> u: U;
      @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
        let p = array(vec2f(-1, -1), vec2f(3, -1), vec2f(-1, 3));
        return vec4f(p[i], 0, 1);
      }
      @fragment fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
        let d = abs(distance(pos.xy, u.centre) - u.radius) - u.stroke * 0.5;
        let a = clamp(0.5 - d, 0.0, 1.0);
        return vec4f(u.color.rgb * a, a);
      }`,
  });
  const pipeline = device.createRenderPipeline({
    layout: 'auto',
    vertex: { module, entryPoint: 'vs' },
    fragment: { module, entryPoint: 'fs', targets: [{ format }] },
    primitive: { topology: 'triangle-list' },
  });
  const uniforms = device.createBuffer({ size: 32, usage: 0x40 | 0x8 }); // UNIFORM | COPY_DST
  const bind = device.createBindGroup({ layout: pipeline.getBindGroupLayout(0), entries: [{ binding: 0, resource: { buffer: uniforms } }] });
  const [r, g, b] = rgb(color);
  const state: RingState = { x: NaN, y: NaN, dirty: true, dpr: 1 };
  const draw = () => {
    const k = state.dpr;
    device.queue.writeBuffer(uniforms, 0, new Float32Array([state.x * k, state.y * k, radius * k, Number.isNaN(state.x) ? 0 : stroke * k, r, g, b, 1]));
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({ colorAttachments: [{ view: context.getCurrentTexture().createView(), loadOp: 'clear', storeOp: 'store', clearValue: { r: 0, g: 0, b: 0, a: 0 } }] });
    pass.setPipeline(pipeline);
    pass.setBindGroup(0, bind);
    pass.draw(3);
    pass.end();
    device.queue.submit([encoder.finish()]);
  };
  const stop = rafLoop(state, draw);
  return {
    move(x, y) {
      state.x = x;
      state.y = y;
      state.dirty = true;
    },
    resize(width, height, dpr) {
      canvas.width = Math.max(1, Math.round(width * dpr));
      canvas.height = Math.max(1, Math.round(height * dpr));
      state.dpr = dpr;
      state.dirty = true;
    },
    destroy() {
      stop();
      context.unconfigure?.();
      device.destroy?.();
    },
  };
}

/** A WebGL2 canvas drawing one ring on rAF; null where WebGL2 is missing. */
export function webglTracker(canvas: HTMLCanvasElement, color: string, radius: number, stroke: number): Tracker | null {
  const gl = canvas.getContext('webgl2', { premultipliedAlpha: true, alpha: true, antialias: false });
  if (!gl) return null;
  const shader = (type: number, src: string) => {
    const s = gl.createShader(type)!;
    gl.shaderSource(s, src);
    gl.compileShader(s);
    return s;
  };
  const program = gl.createProgram()!;
  gl.attachShader(program, shader(gl.VERTEX_SHADER, `#version 300 es
    void main() {
      vec2 p[3] = vec2[3](vec2(-1, -1), vec2(3, -1), vec2(-1, 3));
      gl_Position = vec4(p[gl_VertexID], 0, 1);
    }`));
  gl.attachShader(program, shader(gl.FRAGMENT_SHADER, `#version 300 es
    precision highp float;
    uniform vec2 centre; uniform float radius; uniform float stroke; uniform vec3 color; uniform float height;
    out vec4 o;
    void main() {
      vec2 p = vec2(gl_FragCoord.x, height - gl_FragCoord.y);
      float d = abs(distance(p, centre) - radius) - stroke * 0.5;
      float a = clamp(0.5 - d, 0.0, 1.0);
      o = vec4(color * a, a);
    }`));
  gl.linkProgram(program);
  const at = (n: string) => gl.getUniformLocation(program, n);
  const [r, g, b] = rgb(color);
  const state: RingState = { x: NaN, y: NaN, dirty: true, dpr: 1 };
  const draw = () => {
    const k = state.dpr;
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (Number.isNaN(state.x)) return;
    gl.useProgram(program);
    gl.uniform2f(at('centre'), state.x * k, state.y * k);
    gl.uniform1f(at('radius'), radius * k);
    gl.uniform1f(at('stroke'), stroke * k);
    gl.uniform3f(at('color'), r, g, b);
    gl.uniform1f(at('height'), canvas.height);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };
  const stop = rafLoop(state, draw);
  return {
    move(x, y) {
      state.x = x;
      state.y = y;
      state.dirty = true;
    },
    resize(width, height, dpr) {
      canvas.width = Math.max(1, Math.round(width * dpr));
      canvas.height = Math.max(1, Math.round(height * dpr));
      state.dpr = dpr;
      state.dirty = true;
    },
    destroy() {
      stop();
      gl.getExtension('WEBGL_lose_context')?.loseContext();
    },
  };
}
