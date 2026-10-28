//! Platform-independent runtime: graphics composition, sound synthesis and
//! the virtual file system. Frontends only consume the finished frame and
//! audio buffers.

pub mod anim;
pub mod collision;
pub mod compose;
pub mod confirm;
pub mod dialog;
pub mod editor;
pub mod env;
mod files;
pub mod font;
pub mod framebuffer;
pub mod graphics;
mod grp;
pub mod image;
pub mod input;
mod loadsave;
pub mod machine;
pub mod palette;
mod prg;
pub mod resources;
pub mod runtime;
pub mod sbfile;
pub mod screen;
pub mod settings;
mod snd;
pub mod sound;
mod sp;
pub mod sprites;
pub mod storage;
pub mod syntax;
mod system;
pub mod text;
pub mod transfer;
mod txt;

pub use framebuffer::Framebuffer;
pub use runtime::Runtime;
