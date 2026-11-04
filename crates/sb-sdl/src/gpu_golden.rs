//! Small scenes drawn by the GPU path and the CPU, and compared: one at start-up to
//! decide whether the path can be trusted on this driver, and all of them when
//! `SB_GPU_GOLDEN` is set, printing a row for each. `cargo test -p sb-sdl --test
//! gpu_golden -- --ignored` runs that with no window; CI has no GPU, so it stays
//! ignored there. The CPU is the reference, so no frames are kept anywhere.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use sb_runtime::Runtime;
use sb_runtime::confirm::FileConfirm;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;
use sb_runtime::runtime::keys;
use sb_runtime::storage::Storage;
use sdl3::sys::render::SDL_Renderer;

use crate::gpu::{Compared, Gpu};

/// One scene, and how far it may differ from the CPU.
pub struct Golden {
    pub name: &'static str,
    pub source: &'static str,
    /// A subprogram, started from direct mode once the program ends.
    pub sub: Option<&'static str>,
    /// Typed in direct mode once the program ends.
    pub typed: Option<&'static str>,
    /// The part of the pixels that may be more than 2 off, in any channel: an
    /// element's edges and samples between texels, a blur's rounding.
    pub share: f64,
}

/// How much of the frame the start-up scene may have more than 2 off, among the golden
/// scenes, and more than rounding can explain, at start-up. Its elements sit on whole
/// pixels at whole scales but for one turned, whose edges may differ.
const SELF_TEST_SHARE: f64 = 0.005;

/// What goes on the sprites' page first: colors, a part half clear, a line to see
/// which way up it is, and a pixel of each kind for the filters.
macro_rules! page {
    ($rest:literal) => {
        concat!(
            "GTARGET 4\n",
            "GFILL 0,0,31,31,&HFFFF8040\n",
            "GFILL 8,8,23,23,&H8040C0FF\n",
            "GLINE 0,0,31,15,&HFF00FF00\n",
            "GFILL 32,0,63,31,&HFF2040A0\n",
            "GFILL 40,8,47,15,&H00000000\n",
            "BACKCOLOR &HFF203040\n",
            $rest,
        )
    };
}

/// The scene the path has to pass at start-up: the blends, tints, compose modes,
/// FADE, text on a fallback glyph, a turned element, and each filter that can do
/// without a shader. It has no blur, whose differences are accepted. The checkers at
/// the page's far corner are for a GPU whose fragment shaders hold texture coordinates
/// in 16 bits, as GLES2's mediump may: a texel apart is all such a number can tell
/// there, and sampling goes wrong, where near the page's start it looks right.
pub const SELF_TEST: Golden = Golden {
    name: "self-test",
    source: page!(
        "SPSET 0,0,0,32,32:SPOFS 0,8,8\n\
         SPSET 1,0,0,32,32,16:SPOFS 1,48,8:SPCOLOR 1,&HC0FF80FF\n\
         SPSET 2,32,0,32,32:SPOFS 2,88,8:SPSCALE 2,2,2:SPCOLOR 2,&H80FFFFFF\n\
         SPSET 3,0,0,32,32:SPOFS 3,170,10:SPROT 3,30\n\
         LAYER 1,1,&HC080FFFF:SPSET 4,0,0,64,32:SPOFS 4,8,60:SPLAYER 4,1\n\
         LAYER 2,2:SPSET 5,0,0,64,32:SPOFS 5,80,60:SPLAYER 5,2\n\
         LAYER 3,3:SPSET 6,0,0,64,32:SPOFS 6,152,60:SPLAYER 6,3\n\
         LAYER 4,4:SPSET 7,0,0,64,32:SPOFS 7,224,60:SPLAYER 7,4\n\
         SPSET 8,0,0,32,32:SPOFS 8,8,110:SPLAYER 8,5:LFILTER 5,1,4\n\
         SPSET 9,0,0,32,32:SPOFS 9,48,110:SPLAYER 9,6\n\
         DIM R[8,2]:R[3,0]=0.05:R[3,1]=1:R[4,0]=-0.05:R[4,1]=1:R[0,1]=1:R[1,1]=1\n\
         R[2,1]=1:R[5,1]=1:R[6,1]=1:R[7,1]=1:LFILTER 6,3,R,1\n\
         FOR Y=2016 TO 2047:FOR X=2016 TO 2047\n\
         GPSET X,Y,&HFF000000+&HFFFFFF*((X+Y) AND 1)\n\
         NEXT:NEXT\n\
         SPSET 10,2016,2016,32,32:SPOFS 10,100,110:SPSCALE 10,2,2\n\
         PRINT \"SELF TEST\"\n\
         FADE &H20102030\n"
    ),
    sub: None,
    typed: None,
    share: SELF_TEST_SHARE,
};

/// A 4x4 matrix with true perspective, for a 400x240 screen.
macro_rules! leaning {
    ($rest:literal) => {
        concat!(
            "DIM M#[16]\n",
            "M#[0]=2/400:M#[12]=-1:M#[5]=-2/240:M#[13]=1:M#[10]=1:M#[15]=1:M#[3]=0.002\n",
            "LMATRIX 1,M#\n",
            $rest
        )
    };
}

const fn scene(name: &'static str, source: &'static str, share: f64) -> Golden {
    Golden {
        name,
        source,
        sub: None,
        typed: None,
        share,
    }
}

/// Every rule the path has to keep, one scene a rule or two.
pub const SCENES: &[Golden] = &[
    SELF_TEST,
    scene(
        "sprites",
        page!(
            "SPSET 0,0,0,32,32:SPOFS 0,8,8\n\
             SPSET 1,0,0,32,32,1:SPOFS 1,48,8\n\
             SPSET 2,0,0,32,32,2:SPOFS 2,88,8\n\
             SPSET 3,0,0,32,32,8:SPOFS 3,128,8\n\
             SPSET 4,0,0,32,32,4:SPOFS 4,168,8\n\
             SPSET 5,0,0,32,32,16:SPOFS 5,28,28:SPCOLOR 5,&H80FF8080\n\
             SPSET 6,0,0,32,32:SPOFS 6,8,60:SPSCALE 6,2,2\n\
             SPSET 7,0,0,32,32:SPOFS 7,88,60:SPSCALE 7,1.5,2.5\n"
        ),
        0.001,
    ),
    scene(
        "turned",
        page!(
            "SPSET 0,0,0,32,32:SPOFS 0,60,60:SPROT 0,30\n\
             SPSET 1,0,0,32,32:SPOFS 1,140,60:SPROT 1,45:SPSCALE 1,2,2\n\
             SPSET 2,0,0,32,32:SPOFS 2,240,60:SPROT 2,90\n\
             SPSET 3,0,0,32,32:SPOFS 3,300,60:SPROT 3,17:SPSCALE 3,1.3,0.7\n"
        ),
        0.01,
    ),
    // Where a pixel's center falls exactly between two texels, on half pixels and
    // at 1.5, flipped and turned, sprites and a cell of each size.
    scene(
        "ties",
        "ACLS:GCLS 0:GTARGET 3:FOR Y=0 TO 99:FOR X=0 TO 99\n\
         GPSET X,Y,RGB(40+X*2,40+Y*2,128):NEXT:NEXT:GTARGET 0\n\
         N=0:FOR S=0 TO 2:FOR A=0 TO 15:W=16-(S==1)\n\
         SPSET N,20,20,W,W,A:SPPAGE N,3\n\
         IF S==2 THEN SPSCALE N,1.5,1.5:SPOFS N,4+A*25,180 ELSE SPOFS N,4.5+A*25,8.5+S*30\n\
         N=N+1:NEXT:NEXT\n\
         TSCREEN 0,16,16,16,2:TPAGE 0,3,0,0:TOFS 0,4.5,80.5\n\
         TSCREEN 1,16,16,16,2:TPAGE 1,3,0,0:TOFS 1,4,120:TSCALE 1,1.5,1.5\n\
         FOR A=0 TO 15:TPUT 0,A,0,CHR$(&HE801),A:TPUT 1,A,0,CHR$(&HE801),A:NEXT\n",
        0.01,
    ),
    scene(
        "text",
        "BACKCOLOR &HFF102030\n\
         PRINT \"THE CONSOLE AND SOME TEXT\"\n\
         TSCREEN 0,16,16,20,4:TLAYER 0,1:TOFS 0,16,64\n\
         TPUT 0,1,1,\"QR\":TCOLOR 0,&HC0FFFF80\n",
        0.001,
    ),
    scene(
        "compose",
        page!(
            "LAYER 1,1,&HC080FFFF:SPSET 0,0,0,64,32:SPOFS 0,8,8:SPLAYER 0,1:SPSCALE 0,2,2\n\
             LAYER 2,2,&HFFC0C0C0:SPSET 1,0,0,64,32:SPOFS 1,88,28:SPLAYER 1,2:SPSCALE 1,2,2\n\
             LAYER 3,3,&HFFFFFF80:SPSET 2,0,0,64,32:SPOFS 2,168,48:SPLAYER 2,3:SPSCALE 2,2,2\n\
             LAYER 4,4,&H80FFFFFF:SPSET 3,0,0,64,32:SPOFS 3,248,68:SPLAYER 3,4:SPSCALE 3,2,2\n"
        ),
        0.001,
    ),
    scene(
        "fade and clip",
        page!(
            "SPSET 0,0,0,64,32:SPOFS 0,8,8:SPSCALE 0,4,4:LCLIP 0,20,20,120,90\n\
             FADE &H60FF0000\n"
        ),
        0.001,
    ),
    scene(
        "mosaic",
        page!(
            "LAYER 1,1:SPSET 0,0,0,64,32:SPOFS 0,8,8:SPSCALE 0,4,4:SPLAYER 0,1:LFILTER 1,1,5\n\
             SPSET 1,0,0,64,32:SPOFS 1,100,150:SPSCALE 1,4,2:SPLAYER 1,2:LFILTER 2,1,6\n"
        ),
        0.01,
    ),
    scene(
        "blur",
        page!(
            "LAYER 1,1:SPSET 0,0,0,64,32:SPOFS 0,8,8:SPSCALE 0,4,4:SPLAYER 0,1:LFILTER 1,2,64\n\
             SPSET 1,0,0,64,32:SPOFS 1,100,150:SPSCALE 1,4,2:SPLAYER 1,2:LFILTER 2,2,16\n"
        ),
        // Accepted: nobody compares a blur by the pixel.
        0.05,
    ),
    scene(
        "raster",
        page!(
            "DIM R[240,4]:VAR I\n\
             FOR I=0 TO 239:R[I,0]=SIN(I/20)*0.1:R[I,1]=1:R[I,2]=0:R[I,3]=1:NEXT\n\
             LAYER 1,1:SPSET 0,0,0,64,32:SPOFS 0,8,8:SPSCALE 0,4,4:SPLAYER 0,1:LFILTER 1,3,R,1\n\
             DIM Q[400,2]\n\
             FOR I=0 TO 399:Q[I,0]=COS(I/30)*0.1:Q[I,1]=0.5:NEXT\n\
             SPSET 1,0,0,64,32:SPOFS 1,100,150:SPSCALE 1,4,2:SPLAYER 1,2:LFILTER 2,4,Q,0\n"
        ),
        0.001,
    ),
    scene(
        "color filter",
        page!(
            "LAYER 1,1:SPSET 0,0,0,64,32:SPOFS 0,8,8:SPSCALE 0,4,4:SPLAYER 0,1:LFILTER 1,5,60,40,20\n\
             SPSET 1,0,0,64,32:SPOFS 1,100,150:SPSCALE 1,4,2:SPLAYER 1,2:LFILTER 2,5,180,0,-30\n"
        ),
        0.001,
    ),
    scene(
        "4x4 sprites",
        leaning!(
            "GTARGET 4\nGFILL 0,0,31,31,&HFFFF8040\nGFILL 8,8,23,23,&H8040C0FF\n\
             BACKCOLOR &HFF203040\n\
             LAYER 1,1:SPSET 0,0,0,32,32:SPOFS 0,10,10:SPSCALE 0,5,5:SPLAYER 0,1\n\
             SPSET 1,0,0,32,32:SPOFS 1,200,40:SPSCALE 1,3,3:SPROT 1,20:SPLAYER 1,1\n"
        ),
        0.02,
    ),
    scene(
        "4x4 text",
        leaning!(
            "BACKCOLOR &HFF203040\n\
             TLAYER 4,1\n\
             PRINT \"THROUGH A MATRIX\"\n\
             PRINT \"TWO LINES OF IT\"\n"
        ),
        0.02,
    ),
    scene(
        "4x4 text clipped",
        leaning!(
            "BACKCOLOR &HFF203040\n\
             TLAYER 4,1\n\
             LCLIP 1,40,20,300,60\n\
             PRINT \"THROUGH A MATRIX\"\n\
             PRINT \"TWO LINES OF IT\"\n"
        ),
        0.02,
    ),
    scene(
        "4x4 cut by depth",
        "DIM M#[16]\n\
         M#[0]=2/400:M#[12]=-1:M#[5]=-2/240:M#[13]=1:M#[15]=1:M#[2]=1/60:M#[14]=-2\n\
         LMATRIX 1,M#\n\
         GTARGET 4:GFILL 0,0,31,31,&HFFFF8040\n\
         LAYER 1,1:SPSET 0,0,0,32,32:SPOFS 0,0,10:SPSCALE 0,12,5:SPLAYER 0,1\n",
        0.002,
    ),
    scene(
        "sample 2",
        page!(
            "XSCREEN 400,240,2\n\
             SPSET 0,0,0,32,32:SPOFS 0,8.5,8.5\n\
             LAYER 1,1:SPSET 1,0,0,64,32:SPOFS 1,60,60:SPSCALE 1,1.5,1.5:SPLAYER 1,1\n\
             LFILTER 1,1,3\n"
        ),
        0.01,
    ),
    scene(
        "repeat",
        page!(
            "GSAMPLE 4,1\nSPSET 0,0,0,64,32:SPOFS 0,8,8:SPHOME 0,0,0:SPSCALE 0,1,1\nSPSET 1,1990,2010,120,90:SPOFS 1,100,60\n"
        ),
        0.001,
    ),
    Golden {
        name: "direct mode",
        source: "BACKCOLOR &HFF304050\nPRINT \"A LINE\"\n",
        sub: None,
        typed: Some("SPSET 0"),
        share: 0.0,
    },
    Golden {
        name: "selected",
        source: "BACKCOLOR &HFF304050\nPRINT \"A LINE\"\n",
        sub: None,
        typed: Some("PRINT 1234\u{F8F4}"),
        share: 0.0,
    },
    Golden {
        name: "sub screen",
        source: page!("SPSET 0,0,0,32,32:SPOFS 0,8,8\n"),
        sub: Some(
            "XSUBSCREEN 500,100,640,500\n\
             BACKCOLOR 0\n\
             LAYER 1,1,&HC0FFFFFF:LAYER 2,2:LAYER 3,3:LAYER 4,4\n\
             GTARGET 4:GFILL 0,0,31,31,&HFFFF8040:GFILL 8,8,23,23,&H8040C0FF\n\
             SPSET 0,0,0,32,32:SPOFS 0,8,8\n\
             SPSET 1,0,0,32,32:SPOFS 1,48,8:SPLAYER 1,1\n\
             SPSET 2,0,0,32,32:SPOFS 2,88,8:SPLAYER 2,2\n\
             SPSET 3,0,0,32,32:SPOFS 3,128,8:SPLAYER 3,3\n\
             SPSET 4,0,0,32,32:SPOFS 4,168,8:SPLAYER 4,4\n\
             PRINT \"SUB SCREEN\"\n\
             WHILE 1:VSYNC:WEND\n",
        ),
        typed: None,
        share: 0.001,
    },
    Golden {
        name: "sub dimmed",
        source: page!("SPSET 0,0,0,32,32:SPOFS 0,8,8\nFADE &H40FF0000\n"),
        sub: Some(
            "XSUBSCREEN 100,100,800,300\n\
             ENVFOCUS 0\n\
             BACKCOLOR &HFF204060\n\
             GTARGET 4:GFILL 0,0,31,31,&HFFFF8040\n\
             SPSET 0,0,0,32,32:SPOFS 0,8,8\n\
             PRINT \"SUB SCREEN\"\n\
             WHILE 1:VSYNC:WEND\n",
        ),
        typed: None,
        share: 0.001,
    },
];

/// What a scene came to.
pub struct Outcome {
    pub main: Compared,
    pub sub: Compared,
    /// A program stopped on an error, or a frame stayed on the CPU, so that the
    /// scene says nothing about the path.
    pub trouble: Option<String>,
    pub took: Duration,
}

impl Outcome {
    pub fn passes(&self, share: f64) -> bool {
        self.trouble.is_none() && self.main.within(share) && self.sub.within(share)
    }
}

/// Frames a scene is drawn, once its program is done.
const FRAMES: u32 = 4;

/// Types a line and enters it, for the machine's next frames to see to.
fn enter(m: &mut Machine, line: &str) {
    m.type_text(line);
    m.rt.key_press(keys::ENTER);
}

/// What the console says, if it says the program stopped on an error.
fn error_on_console(m: &Machine) -> Option<String> {
    let console = &m.rt.text[4];
    let (w, _) = console.size();
    (console.cells().chunks(w as usize))
        .map(|row| {
            let text = row.iter().map(|c| c.code).collect::<Vec<u16>>();
            String::from_utf16_lossy(&text).replace('\0', " ")
        })
        .find(|row| row.contains(" in 0:") || row.contains(" in 1:"))
}

/// Runs a scene on a path of its own on `renderer`. `storage` is where a subprogram
/// is saved to start it, which the start-up scene has none of and wants none.
pub fn run(renderer: *mut SDL_Renderer, scene: &Golden, storage: Option<PathBuf>) -> Outcome {
    let start = Instant::now();
    let Some(mut gpu) = Gpu::comparing(renderer) else {
        return Outcome {
            main: Compared::default(),
            sub: Compared::default(),
            trouble: Some("no path".to_string()),
            took: start.elapsed(),
        };
    };
    let mut sub_gpu = scene.sub.and_then(|_| Gpu::comparing(renderer));
    let mut rt = Runtime::new(400, 240, Font::fallback());
    if let Some(storage) = storage {
        rt.storage = Storage::new(storage);
    }
    rt.settings.file_confirm = FileConfirm::Silent;
    let mut m = Machine::new(rt);
    m.boot();
    if let Some(sub) = scene.sub {
        m.set_slot(1, sub.to_string());
        enter(&mut m, "SAVE \"GOLDSUB\",1");
        for _ in 0..30 {
            m.frame();
        }
    }
    m.set_slot(0, scene.source.to_string());
    m.run_slot(0);
    let mut frames = 0;
    // A program here takes few frames; one that never ends is a mistake in it.
    while m.is_running() && frames < 120 {
        m.frame();
        frames += 1;
    }
    if scene.sub.is_some() {
        enter(&mut m, "SUBRUN \"GOLDSUB\"");
    }
    if let Some(typed) = scene.typed {
        m.type_text(typed);
    }
    for _ in 0..FRAMES {
        m.frame();
    }
    let mut trouble = error_on_console(&m);
    if trouble.is_some() {
        return Outcome {
            main: Compared::default(),
            sub: Compared::default(),
            trouble,
            took: start.elapsed(),
        };
    }
    for _ in 0..FRAMES {
        m.frame();
        m.render_screens_with(&mut gpu);
        if let Some(sub_gpu) = sub_gpu.as_mut()
            && let Some(sub) = m.sub_screen_with(Some(sub_gpu))
            && !sub.on_backend
        {
            trouble = Some("the sub screen stayed on the CPU".to_string());
        }
    }
    if !m.rt.frame_on_backend() {
        trouble = Some("the frame stayed on the CPU".to_string());
    }
    if scene.sub.is_some() && !m.sub_shows() {
        trouble = Some("no sub screen".to_string());
    }
    let sub = sub_gpu.as_mut().map(Gpu::take_compared).unwrap_or_default();
    let main = gpu.take_compared();
    Outcome {
        main,
        sub,
        trouble,
        took: start.elapsed(),
    }
}

/// The start-up scene, on a path of its own on `renderer`: whether the path can be
/// trusted, and what is printed if not.
pub fn self_test(renderer: *mut SDL_Renderer) -> Result<Duration, String> {
    let outcome = run(renderer, &SELF_TEST, None);
    match outcome.trouble.is_none() && outcome.main.near(SELF_TEST.share) {
        true => Ok(outcome.took),
        false => Err(format!(
            "{}: {}",
            outcome.trouble.as_deref().unwrap_or("differs from the CPU"),
            outcome.main.summary(),
        )),
    }
}

/// Runs every scene and prints a row for each. Whether all passed.
pub fn report(renderer: *mut SDL_Renderer) -> bool {
    let dir = std::env::temp_dir().join(format!("sb-gpu-golden-{}", std::process::id()));
    let mut all = true;
    println!(
        "scene               took     pass  pixels compared, then off by 1, 2, more; most off"
    );
    for scene in SCENES {
        let outcome = run(renderer, scene, scene.sub.map(|_| dir.clone()));
        let pass = outcome.passes(scene.share);
        all &= pass;
        println!(
            "{:18} {:7.1?} {:5} {}{}{}",
            scene.name,
            outcome.took,
            pass,
            outcome.main.summary(),
            match scene.sub {
                Some(_) => format!("; sub: {}", outcome.sub.summary()),
                None => String::new(),
            },
            outcome
                .trouble
                .map_or(String::new(), |t| format!("  TROUBLE: {t}")),
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
    all
}
