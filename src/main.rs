use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use clap::Parser;
use serde::Serialize;
use tiny_http::{Header, Response, Server};

/// The browser bundle (single IIFE with every Shiki grammar inlined) and the
/// page shell, both produced by `build.rs` via Bun and embedded at compile time.
/// The bundle is embedded gzip-compressed and served with `Content-Encoding:
/// gzip` (the browser decompresses it), keeping the binary small.
const APP_JS_GZ: &[u8] = include_bytes!("../web/dist/app.js.gz");
const INDEX_HTML: &str = include_str!("../web/index.html");

/// Seconds to wait for the browser's readiness beacon before giving up, so the
/// CLI can never hang forever if something goes wrong client-side.
const WATCHDOG_SECS: u64 = 60;

/// Render a diff of two files in your browser using @pierre/diffs.
#[derive(Parser)]
#[command(name = "pdiff", version, about, long_about = None)]
struct Args {
    /// Original ("before") file shown on the left side of the diff
    old: PathBuf,
    /// Updated ("after") file shown on the right side of the diff
    new: PathBuf,
    /// Port to bind on 127.0.0.1 (default: a random free port)
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Don't open the browser automatically; just print the URL
    #[arg(long)]
    no_open: bool,
    /// Wrap long lines instead of scrolling them horizontally
    #[arg(long)]
    wrap: bool,
}

#[derive(Serialize)]
struct FilePayload {
    name: String,
    contents: String,
}

#[derive(Serialize)]
struct Payload {
    old: FilePayload,
    new: FilePayload,
    wrap: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("pdiff: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    let old = read_file(&args.old)?;
    let new = read_file(&args.new)?;

    let payload = Payload {
        old,
        new,
        wrap: args.wrap,
    };
    let html = render_page(&payload)?;

    let server = Server::http(("127.0.0.1", args.port))
        .map_err(|e| format!("could not start local server: {e}"))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|addr| addr.port())
        .ok_or("could not determine bound port")?;
    let url = format!("http://127.0.0.1:{port}/");

    eprintln!("pdiff: serving {} \u{2192} {} at {url}", args.old.display(), args.new.display());

    // Safety net: exit even if the browser never reports back.
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(WATCHDOG_SECS));
        std::process::exit(0);
    });

    if args.no_open {
        eprintln!("pdiff: open {url} in your browser");
    } else if let Err(e) = webbrowser::open(&url) {
        eprintln!("pdiff: couldn't open browser ({e}); open {url} manually");
    }

    let html_header = header("Content-Type", "text/html; charset=utf-8");
    let js_header = header("Content-Type", "text/javascript; charset=utf-8");
    let gzip_header = header("Content-Encoding", "gzip");

    for request in server.incoming_requests() {
        // Strip any query string before matching the path.
        let path = request.url().split('?').next().unwrap_or("/");
        let result = match path {
            "/" => request.respond(Response::from_string(&html).with_header(html_header.clone())),
            "/app.js" => request.respond(
                Response::from_data(APP_JS_GZ)
                    .with_header(js_header.clone())
                    .with_header(gzip_header.clone()),
            ),
            "/__ready" => {
                // The browser has everything and has rendered; ack and stop.
                let _ = request.respond(Response::empty(204));
                break;
            }
            _ => request.respond(Response::empty(404)),
        };
        if let Err(e) = result {
            eprintln!("pdiff: response error: {e}");
        }
    }

    Ok(())
}

fn read_file(path: &PathBuf) -> Result<FilePayload, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(FilePayload {
        // Pass the path as given so the library shows it in the header and
        // infers the syntax-highlighting language from its extension.
        name: path.display().to_string(),
        contents,
    })
}

/// Inject the payload into the HTML shell. The JSON is embedded directly as a JS
/// object literal, so we escape the characters that could break out of the
/// inline `<script>` (`<`, `>`) and the JS line separators (U+2028/U+2029).
fn render_page(payload: &Payload) -> Result<String, String> {
    let json = serde_json::to_string(payload)
        .map_err(|e| format!("failed to serialize payload: {e}"))?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Ok(INDEX_HTML.replace("__PDIFF_DATA__", &json))
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("static header is always valid")
}
