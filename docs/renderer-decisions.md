# Renderer decisions

Why the GPU path draws through SDL_Renderer, with one shader on SDL's `gpu`
driver, and the drawbacks of the other options.

- [renderer.md](renderer.md) describes the renderer as built.
- [renderer-verification.md](renderer-verification.md) has the rules and test
  results.

## The choice

The options were:

- **(a)** SDL_Renderer, with `SDL_RenderGeometry` and SDL's blend modes.
- **(b)** SDL 3.4's `SDL_GPURenderState`, a custom fragment shader inside
  SDL_Renderer.
- **(c)** The SDL3 GPU API directly.
- **(d)** wgpu.

The emulator uses (b) on top of (a), because the UI stays on SDL_Renderer
unchanged. (a) draws everything it can on every driver, and (b) adds only the
color filter.

**(a) reaches furthest and needs no shader.** It has no new dependencies.
SmileBASIC 4 blends with fixed function, so SDL's blend modes give its sums, ±1
where the GPU rounds, as on SmileBASIC 4 itself. `SDL_RenderGeometry` and
`SDL_RenderGeometryRaw` draw textured triangles with per-vertex color, which
covers tint. `SDL_SetRenderTextureAddressMode` gives clamp or wrap, which covers
GSAMPLE.
Compose mode 4 (`ONE_MINUS_DST_COLOR`, `ONE`, `ADD`) and "add without alpha"
come from `SDL_ComposeCustomBlendMode`. Custom blend support varies by driver:
per SDL's header, opengl supports only `ADD` with all factors, opengles2 `ADD`,
`SUBTRACT` and `REV_SUBTRACT`, and software none.

It has two limits:

- `SDL_Vertex.position` is a 2D `SDL_FPoint`: there is no W, so 4x4 LMATRIX
  quads cannot be texture-mapped exactly. Fine subdivision only approximates;
  the path uses a grid with a tolerance of a quarter of a pixel
  ([renderer.md](renderer.md)).
- It has no shader support. Mosaic and blur can be built from draws on half-size
  targets, and raster from strips. Color conversion cannot.

**(b) adds the fragment stage, on fewer machines.** It works only on the `gpu`
render driver (`SDL_GPU_RENDERER`), which runs on Vulkan, Direct3D 12 and Metal.
Machines with only OpenGL, OpenGL ES or D3D11, and the web, do not get it. Other
limits, from `SDL_render.h` in `sdl3-src` 3.4.18:

- Shaders must be supplied compiled, in a format the device accepts: SPIR-V for
  Vulkan, DXIL (or DXBC) for D3D12, MSL or metallib for Metal. That is three
  blobs per shader, built offline (glslang, dxc, spirv-cross or
  SDL_shadercross), then committed or generated at build time; the emulator
  commits what it uses. The renderer must be created with
  `SDL_PROP_RENDERER_CREATE_GPU_SHADERS_SPIRV_BOOLEAN` or its DXIL or MSL form,
  and the shader with `SDL_CreateGPUShader` on the renderer's device
  (`SDL_GetGPURendererDevice`). SDL's Vulkan backend binds combined image
  samplers: naga cannot make that (its GLSL front end refuses `sampler2D`, and
  its output keeps the two apart).
- Only the fragment stage is custom. Vertices are still 2D `SDL_Vertex`, and
  blending is still SDL's blend mode. Only a sum that must be bit-exact would
  need the destination read in the shader. A filter draws its target through the
  shader into another, and they swap. A projected quad works if the shader does
  the inverse homography per pixel, drawn over the quad's screen bounds: the
  same inverse mapping `compose::project_image` does on the CPU.
- The sdl3 crate (0.20) has no safe wrapper for it, so it takes raw
  `sdl3::sys::render` calls.

Only the color filter has a shader, since HSV is more than blending can do and
the other filters needed none. Every driver but `gpu` gets that layer from the
CPU.

## (c) and (d)

The recorded reason for the choice is that the UI stays on SDL_Renderer.

**(c) The SDL3 GPU API** gives full pipelines, integer texture formats and
compute passes, with no new crates. It could blend exactly in integers, and
compute passes could do the blur and color filters. Its drawbacks:

- Reach: the same Vulkan, D3D12 and Metal as (b), so no GL-only machines, GLES
  handhelds without suitable Vulkan, or the web.
- The window: a GPU device that claims the window (`Device::with_window`) and an
  SDL_Renderer cannot both present to it. The UI would have to move to GPU
  textures and quads, or share one device with `SDL_CreateGPURenderer`, which
  would need checking on all three backends.

It also has the same three-format shader problem as (b).

**(d) wgpu** has the widest coverage and one shader language, WGSL, which naga
translates at run time. Its drawbacks:

- Cost: it would be the largest dependency in the workspace, and CONTRIBUTING.md
  asks for a reason for each new one. The numbers are below.
- The window: wgpu owns presentation of the window it draws to, so the UI must
  move to wgpu or be uploaded as textures. SDL textures cannot be shared with
  wgpu without readback.

It also cannot reach GLES2 handhelds, or machines with less than GL 3.3 / GLES
3.0.

Measured in a throwaway crate outside this repository: wgpu 30.0.1 with default
features, plus `pollster`, creating an adapter, a device, a WGSL module and a
2048x2048x6 texture. The probe ran on the NVIDIA Vulkan driver and on Mesa's
lavapipe.

| wgpu cost | Measured |
|---|---|
| Unique crates, `cargo tree -e normal` | 61 on x86_64 Linux, 72 on Windows MSVC, 58 on aarch64 macOS, 44 on wasm32 |
| The whole workspace, for comparison | 43 on Linux |
| Clean release build | 23 s on a 16-thread x86-64 desktop |
| Binary, stripped | 4.7 MB, against about 0.3-0.4 MB for an empty stripped Rust program |

Default features build the dx12, metal, gles, vulkan, wgsl and webgpu backends;
trimming them shrinks both build and binary.

## Coverage

| Target | (a) SDL_Renderer | (b) GPURenderState | (c) SDL GPU | (d) wgpu |
|---|---|---|---|---|
| Windows 10/11 | yes (D3D11/12, GL) | D3D12 or Vulkan | D3D12 or Vulkan | D3D12, Vulkan, GL |
| macOS | yes (Metal) | Metal | Metal | Metal |
| Linux desktop | yes (GL, Vulkan) | Vulkan | Vulkan | Vulkan, GL |
| Older machines with GL 2-3 / D3D9-11 only | yes | no (CPU) | no (CPU) | GL backend needs GL 3.3 / GLES 3.0; else CPU |
| ARM handhelds, GLES 2 | yes (opengles2) | no | no | no |
| ARM handhelds, GLES 3 | yes | Vulkan if present | Vulkan if present and features suffice | GLES3 backend |
| WASM | yes (WebGL via opengles2) | no | no | WebGL2, WebGPU |
| SmileBASIC 4's sums, ±1 where the GPU rounds | yes | yes | yes | yes |
| Bit-exact blending | no | with ping-pong | yes (compute or ping-pong) | yes (ping-pong; compute on native) |
| 4x4 projection exact | no (a grid, within a tolerance) | yes (per-pixel inverse in shader) | yes | yes |
| Filters | mosaic, blur and raster by draws; color on the CPU, that layer only | yes (fragment) | yes | yes |
| New dependencies | none | none (raw sys calls) | none | wgpu tree (61 crates on Linux) |
| Shader toolchain | none | offline, three formats | offline, three formats | none (WGSL) |

Whichever option had been chosen, the CPU rasterizer stays the only path on
machines with no suitable GPU driver. The project puts performance first and
aims to run on old and low-end machines with few resources, so CPU performance
work keeps its value.

## Deciding measurements

### What the CPU costs

These are the costs of `Machine::render` in headless runs:
`smilebasic-check --bench 600 PROJECT PROGRAM` on scratch copies of the
projects, release build, one x86-64 desktop with 16 threads (the renderer is
single-threaded). The tree was at d402543, before later drawing optimizations.
Take the numbers as orders of magnitude.

| Program | Scene | draw mean | median | 99% |
|---|---|---|---|---|
| NEON_DRIFTWAY `MAIN.PRG` | 1280x720; 12 sprites of 896x400 (about 4.3 Mpx); layers 3, 5, 6 Multiply, layer 4 Add, color filters on 5 and 6, blur 384 on direct layer 7 | 11.8 ms | 9.9 ms | 65.9 ms |
| ELE2020 `MAIN.PRG` | 1280x720; about 206 sprites; five text screens | 9.6 ms | 9.4 ms | 12.7 ms |
| FINITI `DEMO-SKYBOX` | 1280x720; six 1024x1024 sprites, each on a layer with a 4x4 LMATRIX (cube faces); `#GSPRITE` | 5.6 ms | 7.2 ms | 9.0 ms |
| SOLIDGUNNER_V `MAIN.PRG` | typical sprite game | 1.15 ms | 1.24 ms | 1.72 ms |
| DVD_BOUNCE `MAIN.PRG` | one moving sprite | 0.51 ms | 0.50 ms | 0.72 ms |

How the costs split:

- They scale with covered pixels times layers. Full-screen composed layers and
  filters are the expensive part: each composed layer clears and composes a 3.5
  MiB buffer, and blur and color filters touch every pixel.
- Small sprite games are cheap.
- Uploading a 1280x720 frame is a further 3.5 MiB a frame.

### The path against the CPU

Measured when the path went in, on one x86-64 desktop with an RTX 3060 Ti, on
the opengl driver through EGL with no window shown: ms for each frame drawn. GPU
is the CPU's side (walking the scene, pages, flush) plus the wait for the GPU
when read back at once, with no vsync and no real present. A window's present is
not in these. Instructions are in billions over the same run, for TPERF's scenes
and ELE2020. [benchmarking.md](benchmarking.md) covers how to measure the path.

| Scene | CPU ms | GPU ms | CPU instr | GPU instr | Peak RSS, CPU, GPU | Pixels 1 off, 2, more, of those compared | Most off |
|---|---|---|---|---|---|---|---|
| TMULTI | 72.8 | 3.3 | 153 | 14 | 313, 362 | 0, 0, 0.15% | 220 |
| SPROT | 29.1 | 0.8 | 80 | 6 | 296, 344 | 0.01%, 0.01%, 0.08% | 255 |
| LCOMP | 37.4 | 0.9 | 112 | 3 | 311, 370 | 44%, 0, 0 | 2 |
| LBLUR | 13.2 | 1.7 | 80 | 4 | 313, 374 | 42%, 20%, 2.0% | 6 |
| LMOSAIC | 6.6 | 0.9 | 33 | 3 | 311, 374 | 1.6%, 0.04%, 0 | 3 |
| LCOLOR | 10.0 | 10.2 | 49 | 52 | 311, 380 | 0, 0, 0 | 0 |
| LMATRIX | 15.4 | 1.1 | 75 | 4 | 310, 367 | 0, 0, 0.17% | 255 |
| LRASTER | 8.8 | 1.1 | 48 | 5 | 311, 368 | 0, 0, 0 | 0 |
| GAME | 2.6 | 0.8 | 22 | 11 | 297, 346 | 1.2%, 0.07%, 0.001% | 3 |
| NEON_DRIFTWAY | 65 | 3.1 | 113 | 26 | 347, 408 | 3.4%, 0.26%, 0.03% | 255 |
| FINITI skybox | 6.8 | 4.2 | 62 | 44 | 359, 409 | 0.67%, 0.32%, 0.60% | 255 |
| SOLIDGUNNER_V | 2.3 | 1.4 | 24 | 9 | 276, 329 | 0.01%, 0, 0.02% | 255 |
| ELE2020 | 13.2 | 2.1 | 143 | 11 | 313, 368 | 5.4%, 0, 0.003% | 255 |

Elements' edges and samples between texels are the 255s. A plain sub screen and
the inverted cells of DIRECT mode cost about 1 ms either way. Measured by hand
in a real window, unlike the headless numbers around it, TMULTI took 2.47 ms for
the screen, and SPROT and TMULTI look as on the CPU there. The other scenes have
not been measured in a window.
