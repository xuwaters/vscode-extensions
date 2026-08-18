//! Write typst's bundled default fonts to a directory.
//!
//! The extension ships these as plain files under `assets/fonts/` rather than
//! embedding them in the WASM artifact: 9.5 MB inside the module would sit in
//! the WASM heap permanently, whereas as files the host reads them through the
//! same callback the VFS uses and only for faces a document actually selects
//! (decision 0004).
//!
//! ```sh
//! cargo run -p typst-session --example dump-fonts -- extensions/typst-ultra/assets/fonts
//! ```

use std::path::Path;

/// The bundled files, in the order `typst_assets::fonts()` yields them.
///
/// `fonts()` returns bytes without names, so the mapping lives here. The length
/// assertion below fails loudly if an upstream bump changes the set, which is
/// also the signal to revisit `extensions/typst-ultra/LICENSE.md` — the font
/// licences are not all the same (`NewCM10-Regular.otf` is GPL with a font
/// exception).
const NAMES: &[&str] = &[
    "LibertinusSerif-Regular.otf",
    "LibertinusSerif-Bold.otf",
    "LibertinusSerif-Italic.otf",
    "LibertinusSerif-BoldItalic.otf",
    "LibertinusSerif-Semibold.otf",
    "LibertinusSerif-SemiboldItalic.otf",
    "NewCMMath-Bold.otf",
    "NewCMMath-Book.otf",
    "NewCMMath-Regular.otf",
    "NewCM10-Regular.otf",
    "NewCM10-Bold.otf",
    "NewCM10-Italic.otf",
    "NewCM10-BoldItalic.otf",
    "DejaVuSansMono-Bold.ttf",
    "DejaVuSansMono-BoldOblique.ttf",
    "DejaVuSansMono-Oblique.ttf",
    "DejaVuSansMono.ttf",
];

fn main() -> std::io::Result<()> {
    let Some(out) = std::env::args().nth(1) else {
        eprintln!("usage: dump-fonts <output-directory>");
        std::process::exit(2);
    };
    let out = Path::new(&out);
    std::fs::create_dir_all(out)?;

    let files: Vec<&'static [u8]> = typst_assets::fonts().collect();
    assert_eq!(
        files.len(),
        NAMES.len(),
        "typst-assets changed its bundled font set — update NAMES and LICENSE.md"
    );

    let mut total = 0usize;
    for (name, bytes) in NAMES.iter().zip(&files) {
        let path = out.join(name);
        // Skip identical rewrites so `build:fonts` stays cheap in a watch loop.
        let unchanged = std::fs::read(&path).is_ok_and(|existing| existing == *bytes);
        if !unchanged {
            std::fs::write(&path, bytes)?;
        }
        total += bytes.len();
    }

    println!(
        "wrote {} fonts ({:.1} MB) to {}",
        files.len(),
        total as f64 / 1_048_576.0,
        out.display()
    );
    Ok(())
}
