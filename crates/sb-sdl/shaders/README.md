# Shaders of the GPU path

The sources are the `.frag` files. `SDL_GPURenderState` takes compiled shaders,
so the compiled `.spv` is committed beside each source and included in the
binary, and building the emulator needs no shader compiler.

To make one again, with `glslangValidator` from the Vulkan SDK or a distribution:

```
glslangValidator -V --target-env vulkan1.0 color_filter.frag -o color_filter.frag.spv
```

`naga` cannot make these: its SPIR-V keeps textures and samplers apart, and SDL's
Vulkan backend binds them combined, as `sampler2D`.

For now there is SPIR-V only, for Vulkan. Direct3D 12 needs DXIL and Metal MSL
as well, as `SDL_shadercross` makes from the same source. See
[renderer-design.md](../../../docs/renderer-design.md), section 6.
