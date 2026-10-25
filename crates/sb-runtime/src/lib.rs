//! Platform-independent runtime: graphics composition, sound synthesis and
//! the virtual file system. Frontends only consume the finished frame and
//! audio buffers.

pub mod framebuffer;

pub use framebuffer::Framebuffer;
