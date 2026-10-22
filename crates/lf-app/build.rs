//! Build script: Tauri's codegen, plus a placeholder settings page when the
//! React UI has not been built.
//!
//! `tauri::generate_context!` embeds `dist/` at compile time, so the crate
//! cannot compile without it. `npm run build` (or `npm run tauri build`)
//! produces the real UI there; a plain `cargo build`/`cargo test` of the
//! workspace gets a page that says how to build it. `dist/` is ignored by git.

use std::fs;
use std::path::Path;

const PLACEHOLDER: &str = r#"<!doctype html>
<html lang="en">
  <head><meta charset="utf-8" /><title>Layout Fixer</title></head>
  <body style="font-family: sans-serif; padding: 2rem">
    <h1>Layout Fixer</h1>
    <p>The settings UI was not built. Run <code>npm ci &amp;&amp; npm run build</code>
    in <code>crates/lf-app</code>, then rebuild the app.</p>
  </body>
</html>
"#;

fn main() {
    let dist = Path::new("dist");
    if !dist.join("index.html").exists() {
        fs::create_dir_all(dist).expect("create dist/");
        fs::write(dist.join("index.html"), PLACEHOLDER).expect("write placeholder UI");
    }
    tauri_build::build()
}
