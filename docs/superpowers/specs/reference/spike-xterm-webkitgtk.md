# Spike: xterm.js in Tauri 2 (WebKitGTK 2.54) vs. Chromium — 2026-10-11

Throwaway spike. Machine: Debian 13, GNOME Wayland, RTX 4070 Ti (+ AMD iGPU), 60 Hz. Tauri 2.12.3 release build, @xterm/xterm 6.0.0, addon-webgl 0.19.0, 4 panes 89x29 in 1440x900, fresh launch per run, 2 repetitions. Chromium baseline: Electron 44.7.0 (WebGL2 via ANGLE on the RTX).

## Recommendation: stay with Tauri

WebGL renderer, default environment (no `WEBKIT_DISABLE_*`). Terminal data via `tauri::ipc::Channel<InvokeResponseBody>` with `InvokeResponseBody::Raw(Vec<u8>)` → `ArrayBuffer` in JS → `term.write(new Uint8Array(buf))`.

## Results

| Config | WebGL OK | Flood 50MB MB/s (r1/r2) | Worst frame in flood (ms) | Redraw fps (r1/r2) | Redraw p95/max (ms) | Dropped frames (>24ms of ~600) |
|---|---|---|---|---|---|---|
| **Tauri WebGL default** | yes | **70.5 / 66.5** | 88 / 105 | 60.2 / 59.8 | 19 / 32–34 | 2 / 2 |
| Tauri WebGL DISABLE_DMABUF_RENDERER=1 | yes | 59.7 / 60.3 | 98 / 106 | 59.8 / 60.1 | 18 / 19–34 | 1 / 0 |
| Tauri WebGL DISABLE_COMPOSITING_MODE=1 | yes | 60.9 / 62.0 | 92 / 108 | 59.1 / 60.0 | 20 / 28–68 | 8 / 1 |
| Tauri WebGL, generator in page (no IPC) | yes | 72.6 / 72.6 | 50 / 61 | 59.7 / 59.8 | 19 / 33 | 2 / 1 |
| Tauri DOM default | n/a | 54.2 / 54.3 | 123 / 114 | 55.7 / 54.5 | 23 / 44–52 | 7 / 14 |
| Tauri DOM DISABLE_DMABUF | n/a | 50.6 / 52.0 | 95 / 94 | 60 / 60 | 23 / 35–38 | 9 / 11 |
| Tauri DOM DISABLE_COMPOSITING | n/a | 51.5 / 51.6 | 118 / 107 | 59.8 / 59.9 | 23 / 36–37 | 8 / 13 |
| **Electron WebGL (Chromium)** | yes | **72.3 / 69.5** | 53 / 50 | 60 / 60 | 16.8 / 17–33 | 1 / 0 |
| Electron DOM | n/a | 57.6 / 58.0 | 67 / 67 | 60 / 60 | 16.8 / 17–18 | 0 / 0 |

## IPC throughput (Rust → JS, 4 panes parallel, 64 KB chunks)

| Encoding | MB/s |
|---|---|
| **`InvokeResponseBody::Raw(Vec<u8>)` → ArrayBuffer** | **300–370** |
| base64 string | 279 |
| UTF-8 JSON string (unsafe for PTY bytes) | 242 |
| `Channel<Vec<u8>>` (JSON number array) — avoid | 76 |

## Observations / guidance

1. 23 launches stable: no crash, no blank window, no GL/DMABUF errors; WebKitWebProcess runs on the NVIDIA GPU.
2. `WEBKIT_DISABLE_*` workarounds not needed; they cost 10–13 % throughput → offer only as opt-in compat setting.
3. DOM renderer ~20 % slower and drops ~4× more frames → fallback only (on WebGL context loss).
4. Chromium advantage: tighter frame pacing and shorter UI hitches during floods (~50 ms vs ~90–110 ms with IPC) — mitigate with batching + flow control.
5. WebGL context limit: 16 live contexts. Attach WebGL only to visible terminals; dispose on hidden tabs; DOM fallback on `onContextLoss`.
6. Not measured: backpressure (implement ack-based flow control with high/low watermarks), CPU, long-run memory, input latency, HiDPI, AMD iGPU path.
