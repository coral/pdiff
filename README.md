# pdiff

CLI tool to diff using the [Pierre diffs library](https://diffs.com/)

```sh
pdiff old.rs new.rs
```

## Usage

```
pdiff <OLD> <NEW> [--wrap] [--port <PORT>] [--no-open]

Arguments:
  <OLD>   Original ("before") file, shown on the left
  <NEW>   Updated ("after") file, shown on the right

Options:
  --wrap          Wrap long lines instead of scrolling them horizontally
  --port <PORT>   Port to bind on 127.0.0.1 (default: a random free port)
  --no-open       Don't open the browser; just print the URL
  -h, --help      Print help
  -V, --version   Print version
```
