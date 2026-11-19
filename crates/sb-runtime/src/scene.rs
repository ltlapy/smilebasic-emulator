//! What a frame shows, as plain data: the scene builder holds SmileBASIC
//! 4's rules for what goes on the screen, in what order, and a rasterizer
//! only draws what it is given. Nothing here knows about SDL, a GPU or the
//! operating system. See docs/rendering/renderer.md.

use std::ops::Range;

use crate::compose::{self, Affine, Compose, LAYERS, Layer};
use crate::filter::{Filter, FilterOp};
use crate::graphics::{Clip, Graphics, PAGES};
use crate::screen::ScreenMode;
use crate::sprites::{Pose, SPRITES, Sprite, Sprites};
use crate::text::{CONSOLE, LineCursor, TextScreen};

/// What makes up a frame, read when its scene is built.
pub struct SceneInput<'a> {
    pub mode: &'a ScreenMode,
    pub layers: &'a [Layer; LAYERS],
    pub filters: &'a [Filter; LAYERS],
    pub sprites: &'a Sprites,
    pub text: &'a [TextScreen],
    pub grp: &'a Graphics,
    /// BACKCOLOR.
    pub back_color: u32,
    /// This is a sub interpreter's screen, which goes over the main one's.
    pub sub: bool,
    /// FADE's color now.
    pub fade: u32,
    /// Darkened beside a subprogram.
    pub dimmed: bool,
    /// The cursor of a line being typed, if it shows: whether it is a block.
    pub cursor: Option<bool>,
}

/// One frame, as a rasterizer draws it. Its vectors are kept from frame to
/// frame, so that building one allocates nothing once they are large enough.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    /// How many frame pixels make one screen pixel.
    pub sample: u32,
    /// What the frame is cleared to.
    pub back: u32,
    /// The frame stays opaque under composed layers, as the main screen's
    /// does all through.
    pub opaque_frame: bool,
    /// The frame is made opaque at the end.
    pub opaque_end: bool,
    /// Layer 7 first, leaving out those with nothing to do.
    pub passes: Vec<LayerPass>,
    /// FADE's color.
    pub fade: u32,
    /// FADE and the darkening beside a subprogram are one step: one
    /// rounding of the two together.
    pub dimmed: bool,
    /// The rows of the raster filters of this frame's passes.
    pub raster_rows: Vec<[f64; 4]>,
    /// All the passes' items back to front, each as a sort key over where
    /// it is in `sprites` or `texts`.
    order: Vec<u64>,
    sprites: Vec<SpriteItem>,
    texts: Vec<TextItem>,
}

/// A layer's part of a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerPass {
    /// [`Compose::Direct`] draws straight on the frame; the others on a
    /// clear layer that is then composed.
    pub compose: Compose,
    /// LAYER's color, for composing.
    pub color: u32,
    /// LCLIP, in frame pixels.
    pub clip: Option<Clip>,
    /// LMATRIX's 4x4 form.
    pub projection: Option<Projection>,
    pub filter: FilterOp,
    /// The frame is made opaque again after the filter: where a raster
    /// filter on a direct layer left nothing, it shows black.
    pub opaque_after_filter: bool,
    /// Where its items are in the scene.
    items: Range<u32>,
}

/// How a layer with LMATRIX's 4x4 form is drawn. Sprites go through the
/// matrix themselves, whole. Text screens are drawn flat on an offscreen
/// frame first, each run of them between sprites put on in its turn: that
/// is why they reach at most one screen past each edge (see
/// docs/using/differences.md).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projection {
    pub matrix: [f64; 16],
    /// The frame pixel at the top left of the flat frame.
    pub flat_origin: (i32, i32),
    pub flat_size: (u32, u32),
}

/// One thing a layer draws.
#[derive(Clone, Copy, Debug)]
pub enum Item<'a> {
    Sprite(&'a SpriteItem),
    Text(&'a TextItem),
}

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

// Within a layer, larger Z is further back; at equal Z text screens go
// behind sprites, and among either, higher ids go behind. A sort key has,
// from the top: layer (7 first), Z, text screens before sprites, id; then
// where the item is, which the rest settles. Sorting these moves far less
// than sorting items.
const KEY_LAYER: u32 = 58;
const KEY_Z: u32 = 26;
const KEY_SPRITE: u64 = 1 << 25;
const KEY_ID: u32 = 13;
const KEY_AT: u64 = (1 << KEY_ID) - 1;

fn sort_key(layer: usize, z: i32, sprite: bool, id: usize, at: usize) -> u64 {
    debug_assert!(layer < LAYERS && id < SPRITES && at < SPRITES);
    let back_first = !((z as u32) ^ 0x8000_0000);
    ((LAYERS - 1 - layer) as u64) << KEY_LAYER
        | u64::from(back_first) << KEY_Z
        | if sprite { KEY_SPRITE } else { 0 }
        | ((SPRITES - 1 - id) as u64) << KEY_ID
        | at as u64
}

impl Scene {
    /// Fills the scene with the frame `input` makes.
    pub fn build(&mut self, input: &SceneInput) {
        let sample = input.mode.sample;
        let frame = input.mode.frame();
        // The sub interpreter's screen goes over the main one's, and so is
        // drawn premultiplied. Its BACKCOLOR's color is put on at full
        // strength, with its alpha as how much of the main screen shows
        // through (coverage 255 less it); one of alpha 0 leaves the
        // screen clear, and an opaque one hides the main screen.
        let back = input.back_color;
        self.back = match (input.sub, back >> 24) {
            (false, _) => back | 0xFF00_0000,
            (_, 0) => 0,
            (_, a) => (255 - a) << 24 | back & 0x00FF_FFFF,
        };
        self.opaque_frame = !input.sub;
        self.opaque_end = input.sub && back >> 24 == 0xFF;
        self.sample = sample;
        self.fade = input.fade;
        self.dimmed = input.dimmed;
        self.passes.clear();
        self.raster_rows.clear();
        self.order.clear();
        self.sprites.clear();
        self.texts.clear();

        let settings: [Layer; LAYERS] = std::array::from_fn(|i| input.layers[i].fine(sample));
        // What ends up in no layer is not drawn.
        let layer_of = |layer: i32| usize::try_from(layer).ok().filter(|&l| l < LAYERS);

        for id in input.sprites.maybe_used() {
            let s = &input.sprites.list()[id];
            if !s.used {
                continue;
            }
            let pose = input.sprites.pose(id);
            let Some(layer) = layer_of(pose.layer).filter(|_| pose.shown) else {
                continue;
            };
            let at = self.sprites.len();
            let matrix = &settings[layer].matrix;
            self.sprites
                .push(SpriteItem::new(s, &pose, matrix, input.grp));
            self.order.push(sort_key(layer, pose.z, true, id, at));
        }

        // Where each layer's text screens are drawn: on the frame, or for a
        // 4x4 matrix on a flat frame that covers them.
        let mut flats = [((0, 0), (0, 0)); LAYERS];
        let mut text_matrix: [Affine; LAYERS] = std::array::from_fn(|i| settings[i].matrix);
        for (layer, s) in settings.iter().enumerate() {
            let Some(m) = &s.projection else { continue };
            let screens =
                (input.text.iter()).filter(|t| t.visible && layer_of(t.layer) == Some(layer));
            let mut extent = None;
            for t in screens {
                let (a, b, c, d) = t.extent(&s.matrix);
                let (x0, y0, x1, y1) = extent.unwrap_or((f64::MAX, f64::MAX, f64::MIN, f64::MIN));
                extent = Some((x0.min(a), y0.min(b), x1.max(c), y1.max(d)));
            }
            let Some(extent) = extent else { continue };
            let shown = compose::source_bounds(m, sample, frame, s.clip.as_ref());
            flats[layer] = flat_area(frame, extent, shown);
            let (x, y) = flats[layer].0;
            let shift = Affine {
                tx: -f64::from(x),
                ty: -f64::from(y),
                ..Affine::IDENTITY
            };
            text_matrix[layer] = s.matrix.then(&shift);
        }
        for (id, t) in input.text.iter().enumerate() {
            let Some(layer) = layer_of(t.layer).filter(|_| t.visible) else {
                continue;
            };
            let composed = settings[layer].compose != Compose::Direct;
            let cursor = input.cursor.filter(|_| id == CONSOLE);
            let cursor = cursor.map(|overwrite| LineCursor {
                overwrite,
                composed,
            });
            let at = self.texts.len();
            self.texts.push(t.item(id, &text_matrix[layer], cursor));
            self.order.push(sort_key(layer, t.z, false, id, at));
        }

        self.order.sort_unstable();

        let mut items = 0;
        for layer in (0..LAYERS).rev() {
            let start = items;
            let top = ((LAYERS - 1 - layer) as u64 + 1) << KEY_LAYER;
            while self.order.get(items).is_some_and(|&key| key < top) {
                items += 1;
            }
            let s = &settings[layer];
            let filter = &input.filters[layer];
            let direct = s.compose == Compose::Direct;
            // A layer that is not composed filters the whole frame as drawn
            // so far, the layers behind and BACKCOLOR included (checked on
            // hardware); a composed one only its own elements.
            // It does so with no elements in the layer.
            if start == items && !(direct && !matches!(filter, Filter::None)) {
                continue;
            }
            self.passes.push(LayerPass {
                compose: s.compose,
                color: s.color,
                clip: s.clip,
                projection: s.projection.map(|matrix| Projection {
                    matrix,
                    flat_origin: flats[layer].0,
                    flat_size: flats[layer].1,
                }),
                filter: filter.op(&mut self.raster_rows),
                // The frame stays opaque where deformation left nothing,
                // which shows black.
                opaque_after_filter: direct && matches!(filter, Filter::Raster { .. }),
                items: start as u32..items as u32,
            });
        }
    }

    /// A pass's items, back to front.
    pub fn items(&self, pass: &LayerPass) -> impl Iterator<Item = Item<'_>> {
        let keys = &self.order[pass.items.start as usize..pass.items.end as usize];
        keys.iter().map(|&key| {
            let at = (key & KEY_AT) as usize;
            match key & KEY_SPRITE {
                0 => Item::Text(&self.texts[at]),
                _ => Item::Sprite(&self.sprites[at]),
            }
        })
    }

    /// The bytes it holds on to.
    pub fn memory(&self) -> usize {
        size_of::<Scene>()
            + self.passes.capacity() * size_of::<LayerPass>()
            + self.raster_rows.capacity() * size_of::<[f64; 4]>()
            + self.order.capacity() * size_of::<u64>()
            + self.sprites.capacity() * size_of::<SpriteItem>()
            + self.texts.capacity() * size_of::<TextItem>()
    }
}

/// Where the flat frame goes that holds text screens covering `extent`,
/// which may be beyond the screen, as its top left frame pixel and its size.
/// It keeps only what `shown` (from [`compose::source_bounds`]) says can
/// show, and reaches at most a screen past each edge, to keep its memory
/// bounded.
fn flat_area(
    (fw, fh): (u32, u32),
    (mut x0, mut y0, mut x1, mut y1): (f64, f64, f64, f64),
    shown: Option<(f64, f64, f64, f64)>,
) -> ((i32, i32), (u32, u32)) {
    let (fw, fh) = (f64::from(fw), f64::from(fh));
    // A pixel to spare each side for where the edges fall.
    let Some((a, b, c, d)) = shown else {
        return ((0, 0), (0, 0));
    };
    (x0, y0) = (x0.max(a - 1.0), y0.max(b - 1.0));
    (x1, y1) = (x1.min(c + 1.0), y1.min(d + 1.0));
    if x0 > x1 || y0 > y1 {
        return ((0, 0), (0, 0));
    }
    let x0 = x0.floor().clamp(-fw, 2.0 * fw) as i32;
    let y0 = y0.floor().clamp(-fh, 2.0 * fh) as i32;
    let x1 = x1.ceil().clamp(-fw, 2.0 * fw) as i32;
    let y1 = y1.ceil().clamp(-fh, 2.0 * fh) as i32;
    ((x0, y0), ((x1 - x0) as u32, (y1 - y0) as u32))
}
