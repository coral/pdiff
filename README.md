# pdiff

CLI tool to diff using the [Pierre diffs library](https://diffs.com/)

```sh
pdiff old.rs new.rs        # diff two files
pdiff HEAD~..HEAD -- src   # diff via git
```

## Usage

```
pdiff [options] <OLD> <NEW>           Diff two files
pdiff [options] [<git diff args>...]  Diff via `git diff`

Options (must come before the diff arguments):
  --wrap          Wrap long lines instead of scrolling them horizontally
  --port <PORT>   Port to bind on 127.0.0.1 (default: a random free port)
  --no-open       Don't open the browser; just print the URL
  -h, --help      Print help
  -V, --version   Print version
```

## Modes

The mode is auto-detected from the arguments:

- **File mode** — exactly two arguments that are both existing files. The two
  files are diffed against each other (left = old, right = new).
- **Git mode** — anything else (a revision range, a single ref, `--staged`, a
  `--` pathspec, or no arguments at all). The arguments are passed straight
  through to `git diff`, and the resulting patch is rendered — every file in the
  diff is shown.

```sh
pdiff old.rs new.rs        # file mode
pdiff HEAD~..HEAD -- src   # git mode: a commit range, limited to src/
pdiff --staged             # git mode: staged changes
pdiff HEAD                  # git mode: working tree vs HEAD
pdiff                       # git mode: unstaged changes
```

To force git mode for two files (instead of diffing them against each other),
pass them after `--`, e.g. `pdiff HEAD -- a.rs b.rs`.
