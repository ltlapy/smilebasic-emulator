//! Platform-independent runtime: graphics composition, sound synthesis and
//! the virtual file system. Frontends only consume the finished frame and
//! audio buffers.

pub mod font;
pub mod framebuffer;
pub mod machine;
pub mod resources;
pub mod runtime;
pub mod sbfile;
pub mod text;

pub use framebuffer::Framebuffer;
pub use runtime::Runtime;
