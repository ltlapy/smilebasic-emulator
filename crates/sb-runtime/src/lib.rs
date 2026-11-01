//! Platform-independent runtime: graphics composition, sound synthesis and
//! the virtual file system. Frontends only consume the finished frame and
//! audio buffers.

pub mod anim;
pub mod collision;
pub mod complete;
pub mod compose;
pub mod confirm;
mod devices;
pub mod dialog;
pub mod editor;
pub mod env;
pub mod error_lines;
mod files;
pub mod filter;
pub mod font;
pub mod framebuffer;
pub mod graphics;
mod grp;
pub mod help;
pub mod hint;
pub mod image;
pub mod input;
pub mod line;
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
mod sub;
pub mod syntax;
mod system;
pub mod text;
pub mod transfer;
mod txt;

pub use framebuffer::Framebuffer;
pub use runtime::Runtime;
pub use system::PerfStats;
