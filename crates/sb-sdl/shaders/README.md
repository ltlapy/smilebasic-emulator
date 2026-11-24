# Shaders of the GPU path

The sources are the `.frag` files. `SDL_GPURenderState` takes compiled shaders,
so the compiled `.spv` is committed next to each source and included in the
binary. Building the emulator needs no shader compiler.

To make one again, with `glslangValidator` from the Vulkan SDK or a distribution:

```
glslangValidator -V --target-env vulkan1.0 color_filter.frag -o color_filter.frag.spv
```

`naga` cannot make these: its SPIR-V keeps textures and samplers separate, and SDL's
Vulkan backend binds them combined, as `sampler2D`.

For now there is SPIR-V only, for Vulkan. Direct3D 12 needs DXIL as well, and
Metal needs MSL; `SDL_shadercross` makes both from the same source. See
[renderer.md](../../../docs/rendering/renderer.md), "Color filter".
