import {
  CodeView,
  type CodeViewItem,
  parseDiffFromFile,
  parsePatchFiles,
} from '@pierre/diffs';

interface FilePayload {
  name: string;
  contents: string;
}

// Mirrors the `Payload` enum in src/main.rs (serde tag = "mode"). File mode
// ships two raw files we diff here; git mode ships a ready-made patch string.
type PdiffPayload =
  | { mode: 'files'; old: FilePayload; new: FilePayload; wrap: boolean }
  | { mode: 'git'; patch: string; title: string; wrap: boolean };

declare global {
  interface Window {
    __PDIFF__?: PdiffPayload;
  }
}

function fail(message: string): never {
  const root = document.getElementById('root');
  if (root != null) {
    root.textContent = message;
    root.setAttribute('data-error', '');
  }
  // Even on failure, let the CLI know it can stop waiting and exit.
  signalReady();
  throw new Error(message);
}

// Compile-time exhaustiveness guard. If a new variant is added to PdiffPayload
// and a `switch (data.mode)` doesn't handle it, `data` is no longer narrowed to
// `never` here and the call fails to type-check.
function assertNever(value: never): never {
  fail(`pdiff: unhandled payload mode: ${JSON.stringify(value)}`);
}

let readySent = false;

/**
 * Tell the Rust CLI that the browser has loaded everything (the bundle and all
 * inlined grammars) and finished its first render, so the CLI can exit. The tab
 * keeps working without the server because all assets are already client-side.
 */
function signalReady(): void {
  if (readySent) return;
  readySent = true;
  try {
    if (navigator.sendBeacon != null) {
      navigator.sendBeacon('/__ready');
      return;
    }
  } catch {
    // fall through to fetch
  }
  // keepalive so the request survives even if the page is navigating/closing.
  void fetch('/__ready', { method: 'POST', keepalive: true }).catch(() => {});
}

// File mode: diff the two raw file contents here. parseDiffFromFile runs jsdiff
// (createTwoFilesPatch) internally and returns FileDiffMetadata, which CodeView
// renders. It throws on degenerate input, so surface a clean message instead of
// a blank page (and still let the CLI exit).
function fileItems(
  data: Extract<PdiffPayload, { mode: 'files' }>
): CodeViewItem[] {
  if (data.old.contents === data.new.contents) {
    fail(`Files are identical — nothing to diff.\n\n${data.old.name}\n${data.new.name}`);
  }
  let fileDiff;
  try {
    fileDiff = parseDiffFromFile(
      { name: data.old.name, contents: data.old.contents },
      { name: data.new.name, contents: data.new.contents }
    );
  } catch (err) {
    fail(`Could not diff the files: ${err instanceof Error ? err.message : String(err)}`);
  }
  return [{ id: `diff:${data.new.name}`, type: 'diff', fileDiff }];
}

// Git mode: parse the raw `git diff` patch into per-file metadata. A single
// patch can hold many files; each becomes its own diff item.
function gitItems(
  data: Extract<PdiffPayload, { mode: 'git' }>
): CodeViewItem[] {
  let patches;
  try {
    patches = parsePatchFiles(data.patch);
  } catch (err) {
    fail(`Could not parse the git diff: ${err instanceof Error ? err.message : String(err)}`);
  }
  const items: CodeViewItem[] = patches.flatMap((patch, pi) =>
    patch.files.map((fileDiff, fi) => ({
      id: `diff:${pi}:${fi}:${fileDiff.name}`,
      type: 'diff' as const,
      fileDiff,
    }))
  );
  if (items.length === 0) {
    fail(`No changes — nothing to diff.\n\n${data.title}`);
  }
  return items;
}

function main(): void {
  const data = window.__PDIFF__;
  if (data == null) {
    fail('pdiff: no payload found on window.__PDIFF__');
  }

  const root = document.getElementById('root');
  if (root == null) {
    fail('pdiff: #root element missing');
  }

  // High-level viewer: owns the scroll container, sticky header, and
  // virtualization. `themeType: 'system'` follows the OS light/dark setting and
  // uses the library's default themes (pierre-light / pierre-dark) — we never
  // register or request any other theme.
  const viewer = new CodeView({
    themeType: 'system',
    stickyHeaders: true,
    // 'wrap' wraps long lines; 'scroll' (the default) scrolls them horizontally.
    overflow: data.wrap ? 'wrap' : 'scroll',
    layout: { paddingTop: 16, paddingBottom: 16, gap: 12 },
    // Fires after the first committed render/hydration of the diff container —
    // a reliable "everything is loaded and painted" signal for the CLI to exit.
    onPostRender(_node, _instance, phase) {
      if (phase === 'mount') {
        signalReady();
      }
    },
  });
  viewer.setup(root);

  let items: CodeViewItem[];
  switch (data.mode) {
    case 'files':
      items = fileItems(data);
      document.title = `${data.old.name} → ${data.new.name} — pdiff`;
      break;
    case 'git':
      items = gitItems(data);
      document.title = `${data.title} — pdiff`;
      break;
    default:
      assertNever(data);
  }
  viewer.setItems(items);

  // Fallbacks in case onPostRender's 'mount' phase never fires: signal once the
  // window has loaded, and again after a short grace period no matter what.
  if (document.readyState === 'complete') {
    setTimeout(signalReady, 500);
  } else {
    window.addEventListener('load', () => setTimeout(signalReady, 500), {
      once: true,
    });
  }
}

main();
