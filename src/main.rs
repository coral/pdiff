use std::path::Path;
use std::process::{Command, ExitCode};
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

/// Render a diff in your browser using @pierre/diffs.
///
/// The mode is auto-detected: exactly two existing files is file mode (the two
/// files are diffed against each other); anything else — a revision range, a
/// single ref, --staged, a `--` pathspec, or no arguments at all — is passed
/// straight through to `git diff`.
///
/// pdiff's own options must come before the diff arguments, since everything
/// after the first positional argument is forwarded verbatim to `git diff`.
#[derive(Parser)]
#[command(
    name = "pdiff",
    version,
    after_help = "\
EXAMPLES:
  pdiff old.rs new.rs        Diff two files
  pdiff HEAD~..HEAD -- src   Diff a commit range, limited to src/
  pdiff --staged             Diff staged changes
  pdiff --wrap abc123 def456 Diff two commits, wrapping long lines"
)]
struct Args {
    /// Port to bind on 127.0.0.1 (default: a random free port)
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Don't open the browser automatically; just print the URL
    #[arg(long)]
    no_open: bool,
    /// Wrap long lines instead of scrolling them horizontally
    #[arg(long)]
    wrap: bool,
    /// Two files to diff, or arguments forwarded verbatim to `git diff`
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    spec: Vec<String>,
}

#[derive(Serialize)]
struct FilePayload {
    name: String,
    contents: String,
}

/// What gets injected into the page. The `mode` tag tells the client whether to
/// diff two raw files itself or to parse a ready-made git patch.
#[derive(Serialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
enum Payload {
    Files {
        old: FilePayload,
        new: FilePayload,
        wrap: bool,
    },
    Git {
        /// Raw `git diff` output; the client parses it with `parsePatchFiles`.
        patch: String,
        /// Human-readable description of the diff (used in the page title).
        title: String,
        wrap: bool,
    },
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

/// How the spec is interpreted, decided once up front.
enum Mode<'a> {
    /// Diff two raw files against each other (left = old, right = new).
    Files { old: &'a str, new: &'a str },
    /// Pass the spec straight through to `git diff`.
    Git,
}

/// File mode applies only when the spec is exactly two existing files with no
/// `--` separator; everything else is treated as a git diff. Binding the two
/// paths into the `Files` variant keeps the "exactly two files" invariant and
/// the paths together, so callers never re-index the spec.
fn detect_mode(spec: &[String]) -> Mode<'_> {
    if let [old, new] = spec
        && old != "--"
        && new != "--"
        && Path::new(old).is_file()
        && Path::new(new).is_file()
    {
        return Mode::Files { old, new };
    }
    Mode::Git
}

fn run(args: Args) -> Result<(), String> {
    let (payload, label) = match detect_mode(&args.spec) {
        Mode::Files { old, new } => {
            let label = format!("{old} \u{2192} {new}");
            (
                Payload::Files {
                    old: read_file(Path::new(old))?,
                    new: read_file(Path::new(new))?,
                    wrap: args.wrap,
                },
                label,
            )
        }
        Mode::Git => {
            let (patch, title) = git_diff(&args.spec)?;
            let label = title.clone();
            (
                Payload::Git {
                    patch,
                    title,
                    wrap: args.wrap,
                },
                label,
            )
        }
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

    eprintln!("pdiff: serving {label} at {url}");

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

fn read_file(path: &Path) -> Result<FilePayload, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(FilePayload {
        // Pass the path as given so the library shows it in the header and
        // infers the syntax-highlighting language from its extension.
        name: path.display().to_string(),
        contents,
    })
}

/// Run `git diff <spec>` verbatim and return its patch plus a label. `--no-color`
/// keeps ANSI codes out of the patch in case the user's git config forces color.
fn git_diff(spec: &[String]) -> Result<(String, String), String> {
    let output = Command::new("git")
        .arg("diff")
        .arg("--no-color")
        .args(spec)
        .output()
        .map_err(|e| format!("failed to run git (is it installed and on PATH?): {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            return Err(format!("git diff exited with {}", output.status));
        }
        return Err(format!("git diff failed: {stderr}"));
    }

    let patch = String::from_utf8_lossy(&output.stdout).into_owned();
    let title = if spec.is_empty() {
        "git diff".to_string()
    } else {
        format!("git diff {}", spec.join(" "))
    };
    Ok((patch, title))
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
