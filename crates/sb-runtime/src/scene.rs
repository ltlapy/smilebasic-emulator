//! What a frame shows, as plain data.

use crate::compose::Affine;
use crate::graphics::{Graphics, PAGES};
use crate::sprites::{Pose, Sprite};

/// A sprite as it is drawn, its links followed and its place worked out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteItem {
    /// From the sprite's shown rectangle to the frame's pixels, through
    /// its layer's 2D matrix.
    pub matrix: Affine,
    /// SPCOLOR with links followed; a clear one is an item that shows
    /// nothing.
    pub color: u32,
    /// Roughly how many pixels the sprite covers.
    pub area: u32,
    /// The graphic page; negative for GRP-1, white all over.
    pub page: i32,
    /// GSAMPLE 1 on the page: it repeats in every direction. Always so
    /// for GRP-1.
    pub repeat: bool,
    /// The image's rectangle on the page.
    pub u: i32,
    pub v: i32,
    pub w: i32,
    pub h: i32,
    /// SPSET's attributes: quarter turns, flips and additive
    /// ([`crate::sprites::attr`]).
    pub attr: i32,
}

impl SpriteItem {
    /// A sprite that shows nothing. It stays in the list because its place
    /// there can still matter: it ends a run of text screens on a layer
    /// with a 4x4 matrix.
    pub const NOTHING: SpriteItem = SpriteItem {
        matrix: Affine::IDENTITY,
        color: 0,
        area: 0,
        page: 0,
        repeat: false,
        u: 0,
        v: 0,
        w: 0,
        h: 0,
        attr: 0,
    };

    /// `layer` is the layer's 2D matrix, already for the frame's pixels.
    pub fn new(s: &Sprite, pose: &Pose, layer: &Affine, grp: &Graphics) -> SpriteItem {
        // A blank page shows nothing, as GRP0 does through #GSPRITE until
        // something is drawn there, and a clear color makes every pixel
        // clear.
        let page = usize::try_from(s.page);
        if page.is_ok_and(|p| p >= PAGES || grp.is_blank(p)) || pose.color >> 24 == 0 {
            return SpriteItem::NOTHING;
        }
        // The shown rectangle has the same area whichever way it is turned.
        let area = (f64::from(s.w * s.h) * (pose.scale.0 * pose.scale.1).abs()).min(1e7);
        SpriteItem {
            matrix: Affine::place(pose.pos, s.home, pose.rot, pose.scale).then(layer),
            color: pose.color,
            area: area as u32,
            page: s.page,
            repeat: page.is_err() || page.is_ok_and(|p| grp.sample[p] == 1),
            u: s.u,
            v: s.v,
            w: s.w,
            h: s.h,
            attr: s.attr,
        }
    }

    pub fn shows_nothing(&self) -> bool {
        self.color >> 24 == 0
    }
}

/// A text screen as it is drawn. Its cells stay where they are, at
/// [`TextItem::screen`]: copying them, or an item for each, would cost
/// more than the whole of drawing some scenes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextItem {
    /// Which of the text screens it is.
    pub screen: u8,
    /// From the screen's pixels to the frame's, through its layer's 2D
    /// matrix.
    pub matrix: Affine,
    /// Nothing to draw: no character and no cursor.
    pub empty: bool,
    /// TSCREEN's font type: the size of the characters' images.
    pub font_type: u32,
    /// What size each cell is, in the screen's pixels.
    pub char_size: u32,
    /// In cells.
    pub width: u32,
    pub height: u32,
    /// TPAGE: where user-defined characters come from.
    pub page: i32,
    pub page_origin: (i32, i32),
    /// TCOLOR.
    pub tint: u32,
    /// TBLEND 1: additive.
    pub add: bool,
    /// An inverted cell is filled around its glyph.
    pub inverts: bool,
    /// A cell may be inverted, which even an empty one shows.
    pub may_invert: bool,
    /// The cursor of a line being typed, over the characters.
    pub cursor: Option<CursorBar>,
}

/// The cursor of a line being typed, a solid box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorBar {
    /// Its cell, from left, top to right, bottom, in the screen's pixels.
    pub part: (i32, i32, i32, i32),
    /// How wide it is from the cell's left.
    pub width: u32,
    pub color: u32,
}
