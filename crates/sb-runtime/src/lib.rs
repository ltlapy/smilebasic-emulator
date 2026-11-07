//! Platform-independent runtime: graphics composition, sound synthesis and
//! the virtual file system. Frontends only consume the finished frame and
//! audio buffers.

pub mod anim;
mod cell_glyph;
pub mod collision;
pub mod complete;
pub mod compose;
pub mod confirm;
mod devices;
pub mod dialog;
pub mod editor;
pub mod env;
pub mod error_lines;
pub mod extra_glyphs;
mod files;
pub mod filter;
pub mod font;
pub mod framebuffer;
pub mod graphics;
mod grp;
pub mod half_width;
pub mod help;
pub mod hint;
pub mod image;
pub mod input;
pub mod line;
mod loadsave;
pub mod machine;
pub mod palette;
mod prg;
mod program_font;
pub mod raster;
pub mod resources;
pub mod runtime;
pub mod sbfile;
pub mod scene;
pub mod screen;
pub mod settings;
pub mod smart_scale;
mod snd;
pub mod sound;
mod sp;
pub mod speed;
pub mod sprites;
pub mod storage;
mod sub;
pub mod syntax;
mod system;
pub mod text;
pub mod transfer;
mod txt;
pub mod ui_glyphs;

pub use files::program_text;
pub use framebuffer::Framebuffer;
pub use runtime::Runtime;
pub use system::PerfStats;
