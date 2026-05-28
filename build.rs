use std::io::Write;
use std::path::Path;
use std::process::Command;

use flate2::Compression;
use flate2::write::GzEncoder;

// Build the browser bundle with Bun before compiling the crate. The resulting
// single-file IIFE (web/dist/app.js) is embedded into the binary via
// include_str! in src/main.rs.
fn main() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let web_dir = Path::new(manifest_dir).join("web");

    // Rebuild the bundle whenever the web sources or deps change.
    println!("cargo:rerun-if-changed=web/src");
    println!("cargo:rerun-if-changed=web/package.json");
    println!("cargo:rerun-if-changed=web/index.html");
    println!("cargo:rerun-if-changed=build.rs");

    let bun = which_bun();

    // Ensure dependencies are installed (idempotent; fast when up to date).
    if !web_dir.join("node_modules").exists() {
        run(
            Command::new(&bun)
                .arg("install")
                .current_dir(&web_dir),
            "bun install",
        );
    }

    // Bundle to a single IIFE so every Shiki grammar is inlined (no runtime
    // dynamic imports / chunk files) and the whole thing is one embeddable file.
    run(
        Command::new(&bun)
            .args([
                "build",
                "src/entry.ts",
                "--format=iife",
                "--target=browser",
                "--minify",
                "--outfile=dist/app.js",
            ])
            .current_dir(&web_dir),
        "bun build",
    );

    let bundle = web_dir.join("dist").join("app.js");
    if !bundle.exists() {
        panic!("bun build did not produce {}", bundle.display());
    }

    // Gzip the bundle so it's embedded compressed (~4-5x smaller). It's served
    // with `Content-Encoding: gzip`, so the browser decompresses it — no runtime
    // cost on the Rust side, and the binary shrinks accordingly. The minified
    // JS is mostly repetitive grammar text, so it compresses very well.
    let js = std::fs::read(&bundle).expect("read app.js");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&js).expect("gzip app.js");
    let gz = encoder.finish().expect("finish gzip");
    let gz_path = web_dir.join("dist").join("app.js.gz");
    std::fs::write(&gz_path, &gz).expect("write app.js.gz");
}

fn which_bun() -> String {
    std::env::var("BUN").unwrap_or_else(|_| "bun".to_string())
}

fn run(cmd: &mut Command, what: &str) {
    let status = cmd.status().unwrap_or_else(|e| {
        panic!(
            "failed to run `{what}` (is Bun installed and on PATH? set BUN=/path/to/bun to override): {e}"
        )
    });
    if !status.success() {
        panic!("`{what}` failed with status {status}");
    }
}
