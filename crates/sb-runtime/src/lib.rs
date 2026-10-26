//! Platform-independent runtime: graphics composition, sound synthesis and
//! the virtual file system. Frontends only consume the finished frame and
//! audio buffers.

pub mod anim;
pub mod collision;
pub mod compose;
pub mod confirm;
pub mod editor;
mod files;
pub mod font;
pub mod framebuffer;
pub mod graphics;
mod grp;
pub mod machine;
pub mod nso;
pub mod resources;
pub mod runtime;
pub mod sbfile;
pub mod screen;
pub mod settings;
mod sp;
pub mod sprites;
pub mod storage;
pub mod text;

pub use framebuffer::Framebuffer;
pub use runtime::Runtime;
