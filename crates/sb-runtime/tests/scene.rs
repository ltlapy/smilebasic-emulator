//! What the scene builder decides, without drawing anything: the order of
//! items, which layers get a pass, and what it costs.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use sb_core::value::{Array, ArrayData, ElemType};
use sb_runtime::compose::{Compose, LAYERS, Layer};
use sb_runtime::filter::{Filter, FilterOp};
use sb_runtime::graphics::{Blend, Clip, Graphics};
use sb_runtime::scene::{Item, Scene, SceneInput};
use sb_runtime::screen::ScreenMode;
use sb_runtime::sprites::{SPRITES, Sprites};
use sb_runtime::text::{CONSOLE, SCREENS, TextScreen};

/// Counts allocations, to see that a frame's scene makes none.
struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: passes everything on to the system allocator unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's obligations are the same as ours.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's obligations are the same as ours.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// A frame's state: no sprite in use, every text screen hidden.
struct World {
    mode: ScreenMode,
    layers: [Layer; LAYERS],
    filters: [Filter; LAYERS],
    sprites: Sprites,
    text: Vec<TextScreen>,
    grp: Graphics,
}

impl World {
    fn new() -> World {
        let mut sprites = Sprites::default();
        for s in &mut sprites.list {
            s.used = false;
        }
        let mut text: Vec<_> = (0..SCREENS).map(|_| TextScreen::new(1280, 720)).collect();
        for t in &mut text {
            t.visible = false;
        }
        let mut grp = Graphics::default();
        grp.target = 4;
        grp.fill(0, 0, 15, 15, 0xFFFF_FFFF, Blend::Normal);
        World {
            mode: ScreenMode::new(1280, 720),
            layers: [Layer::default(); LAYERS],
            filters: Default::default(),
            sprites,
            text,
            grp,
        }
    }

    fn sprite(&mut self, id: usize, layer: i32, z: i32) {
        let s = &mut self.sprites.list[id];
        (s.used, s.shown, s.page, s.layer, s.z) = (true, true, 4, layer, z);
        (s.w, s.h) = (16, 16);
    }

    fn screen(&mut self, id: usize, layer: i32, z: i32) {
        let t = &mut self.text[id];
        (t.visible, t.layer, t.z) = (true, layer, z);
    }

    fn input(&self) -> SceneInput<'_> {
        SceneInput {
            mode: &self.mode,
            layers: &self.layers,
            filters: &self.filters,
            sprites: &self.sprites,
            text: &self.text,
            grp: &self.grp,
            back_color: 0xFF00_0000,
            sub: false,
            fade: 0,
            dimmed: false,
            cursor: None,
        }
    }

    fn scene(&self) -> Scene {
        let mut scene = Scene::default();
        scene.build(&self.input());
        scene
    }
}

/// Each pass's items, as a text screen's number or a sprite's color.
fn listed(scene: &Scene) -> Vec<Vec<i64>> {
    (scene.passes.iter())
        .map(|pass| {
            (scene.items(pass))
                .map(|item| match item {
                    Item::Sprite(s) => i64::from(s.u),
                    Item::Text(t) => -1 - i64::from(t.screen),
                })
                .collect()
        })
        .collect()
}

#[test]
fn items_go_by_layer_then_z_then_text_first_then_higher_ids() {
    let mut w = World::new();
    for (id, layer, z) in [
        (0, 0, 0),
        (1, 0, 0),
        (2, 0, 5),
        (3, 0, -5),
        (4, 3, 0),
        (5, 7, 100),
    ] {
        w.sprite(id, layer, z);
        // Told apart by the image's place.
        w.sprites.list[id].u = id as i32;
    }
    w.screen(0, 0, 0);
    w.screen(1, 0, 0);
    w.screen(2, 7, 200);
    assert_eq!(
        listed(&w.scene()),
        vec![vec![-3, 5], vec![4], vec![2, -2, -1, 1, 0, 3]]
    );
}

#[test]
fn z_at_the_ends_of_its_range_sorts_right() {
    let mut w = World::new();
    for (id, z) in [(0, i32::MAX), (1, i32::MIN), (2, -1), (3, 0)] {
        w.sprite(id, 0, z);
        w.sprites.list[id].u = id as i32;
    }
    assert_eq!(listed(&w.scene()), vec![vec![0, 3, 2, 1]]);
}

/// A linked child takes its parent's Z and layer in the order.
#[test]
fn links_are_followed() {
    let mut w = World::new();
    w.sprite(0, 2, 10);
    w.sprite(1, 0, 5);
    w.sprites.list[1].parent = Some(0);
    w.sprites.list[1].link_flags = sb_runtime::sprites::link::LAYER;
    w.sprites.list[1].u = 1;
    let scene = w.scene();
    // Z 15 goes behind the parent's 10, on the parent's layer.
    assert_eq!(listed(&scene), vec![vec![1, 0]]);
}

/// A sprite that shows nothing still makes its composed layer a pass, which
/// clears and composes, and stays between the text screens of a 4x4 layer.
#[test]
fn sprites_that_show_nothing_keep_their_place() {
    let mut w = World::new();
    w.layers[1].compose = Compose::Multiply;
    w.sprite(0, 1, 0);
    w.sprites.list[0].page = 0; // Blank.
    let scene = w.scene();
    assert_eq!(scene.passes.len(), 1);
    assert_eq!(scene.passes[0].compose, Compose::Multiply);
    let items: Vec<_> = scene.items(&scene.passes[0]).collect();
    assert!(matches!(items[..], [Item::Sprite(s)] if s.shows_nothing()));

    let mut w = World::new();
    w.layers[0].projection = Some(std::array::from_fn(|i| f64::from(i32::from(i % 5 == 0))));
    w.screen(0, 0, 10);
    w.sprite(0, 0, 5);
    w.sprites.list[0].color = 0x00FF_FFFF; // Clear.
    w.screen(1, 0, 0);
    let scene = w.scene();
    assert_eq!(listed(&scene), vec![vec![-1, 0, -2]]);
}

#[test]
fn only_layers_with_something_to_do_get_a_pass() {
    let mut w = World::new();
    assert!(w.scene().passes.is_empty());
    // A filter on a direct layer works on the frame so far, with nothing
    // of its own; one on a composed layer has nothing to work on.
    w.filters[2] = Filter::Blur(4);
    w.filters[3] = Filter::Mosaic(4);
    w.layers[3].compose = Compose::Add;
    let scene = w.scene();
    assert_eq!(scene.passes.len(), 1);
    assert_eq!(scene.passes[0].filter, FilterOp::Blur(4));
}

/// LCLIP and the matrix are in the frame's pixels, finer under XSCREEN's
/// sampling.
#[test]
fn clip_and_items_are_in_frame_pixels() {
    let mut w = World::new();
    w.mode.sample = 2;
    w.layers[0].clip = Some(Clip::new(1, 2, 3, 4));
    w.sprite(0, 0, 0);
    w.sprites.list[0].pos = (10.0, 20.0);
    let scene = w.scene();
    assert_eq!(scene.passes[0].clip, Some(Clip::new(2, 4, 7, 9)));
    let Some(Item::Sprite(s)) = scene.items(&scene.passes[0]).next() else {
        panic!("no sprite");
    };
    assert_eq!((s.matrix.a, s.matrix.tx, s.matrix.ty), (2.0, 20.0, 40.0));
}

#[test]
fn a_raster_filter_keeps_its_rows_as_they_were() {
    let mut w = World::new();
    let mut info = Array::new(ElemType::Real, vec![2, 2]);
    info.data = ArrayData::Real(vec![0.25, 1.0, 0.5, 2.0]);
    let info = std::rc::Rc::new(std::cell::RefCell::new(info));
    w.filters[0] = Filter::Raster {
        vertical: false,
        info: info.clone(),
        repeat: false,
    };
    let scene = w.scene();
    info.borrow_mut().data = ArrayData::Real(vec![9.0; 4]);
    assert_eq!(
        scene.raster_rows,
        vec![[0.25, 1.0, 0.0, 1.0], [0.5, 2.0, 0.0, 1.0]]
    );
    assert!(scene.passes[0].opaque_after_filter);
}

#[test]
fn only_the_console_has_the_cursor() {
    let mut w = World::new();
    w.screen(0, 0, 0);
    w.screen(CONSOLE, 2, 0);
    w.layers[2].compose = Compose::Add;
    let mut scene = Scene::default();
    scene.build(&SceneInput {
        cursor: Some(false),
        ..w.input()
    });
    let bar = |scene: &Scene, pass: usize| {
        let item = scene.items(&scene.passes[pass]).next();
        match item {
            Some(Item::Text(t)) => t.cursor,
            _ => panic!("no text screen"),
        }
    };
    // In a composed layer it adds light and no coverage.
    let console = bar(&scene, 0).unwrap();
    assert_eq!(console.color, 0x00FF_FFFF);
    assert_eq!((console.part, console.width), ((0, 0, 8, 8), 2));
    assert_eq!(bar(&scene, 1), None);
}

#[test]
fn a_sub_interpreters_back_color_is_premultiplied_coverage() {
    let w = World::new();
    let back = |back_color| {
        let mut scene = Scene::default();
        scene.build(&SceneInput {
            back_color,
            sub: true,
            ..w.input()
        });
        (scene.back, scene.opaque_frame, scene.opaque_end)
    };
    assert_eq!(back(0x0012_3456), (0, false, false));
    assert_eq!(back(0x4012_3456), (0xBF12_3456, false, false));
    assert_eq!(back(0xFF12_3456), (0x0012_3456, false, true));
    assert_eq!(w.scene().back, 0xFF00_0000);
    assert!(w.scene().opaque_frame);
}

/// The hardest case for the builder: every sprite in use and shown, some
/// turned and linked, and all five text screens to look through.
fn busy() -> World {
    let mut w = World::new();
    for id in 0..SPRITES {
        w.sprite(id, (id % LAYERS) as i32, (id % 97) as i32);
        let s = &mut w.sprites.list[id];
        s.pos = (id as f64 * 0.25, id as f64 * 0.5);
        s.rot = (id % 360) as f64;
        s.scale = (1.5, 0.5);
        if id % 16 == 1 {
            s.parent = Some(id - 1);
        }
    }
    for id in 0..SCREENS {
        w.screen(id, id as i32, 0);
    }
    w
}

#[test]
fn a_scene_allocates_nothing_once_its_vectors_are_large_enough() {
    let w = busy();
    let input = w.input();
    let mut scene = Scene::default();
    scene.build(&input);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    scene.build(&input);
    let made = ALLOCATIONS.load(Ordering::Relaxed) - before;
    assert_eq!(made, 0);
}

/// Prints what building the largest scene costs. In release:
/// `cargo test --release -p sb-runtime --test scene -- --ignored --nocapture`
#[test]
#[ignore = "a measurement, not a check"]
fn building_the_largest_scene() {
    let w = busy();
    let input = w.input();
    let mut scene = Scene::default();
    scene.build(&input);
    let mut times: Vec<_> = (0..1000)
        .map(|_| {
            let start = std::time::Instant::now();
            scene.build(&input);
            start.elapsed()
        })
        .collect();
    times.sort();
    println!(
        "{} sprites, {} text screens with {} cells each, all empty",
        SPRITES,
        SCREENS,
        w.text[0].cells().len(),
    );
    println!(
        "min {:?}, median {:?}, max {:?}",
        times[0], times[500], times[999]
    );
    println!(
        "scene: {} bytes, of which an item for each sprite {} bytes",
        scene.memory(),
        size_of::<sb_runtime::scene::SpriteItem>(),
    );
}
