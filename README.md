# Launcher

A small macOS application launcher built with Rust and Iced. Runs as a native
window without XQuartz. Iced draws its own widgets; this is not an AppKit UI.

## Run

Install a current stable Rust toolchain and Apple's command-line developer tools,
then run:

```sh
cargo run --release
```

Type to fuzzy-search application names. Use Up/Down to select, Enter to open,
Escape to quit, or click a result. Successful launches close the launcher.
Applications are opened with `/usr/bin/open`, without a shell.

At startup it recursively scans `/Applications` and `$HOME/Applications`, follows
symlinks with cycle detection, deduplicates resolved paths, and stops descending
when it finds an `.app` bundle. Names come from bundle filenames. Missing folders
are ignored; other scan failures are reported in the terminal and UI.

The launcher runs on demand, with no background service or filesystem watcher.
App packaging and an AeroSpace global shortcut are described below. Restart to
refresh the index.
`/System/Applications` is outside the initial search scope.

## Check

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Icons are resolved from `CFBundleIconFile` in XML or binary `Contents/Info.plist`,
with an implicit `.icns` extension when omitted. macOS `sips` converts them in
the background; temporary files are removed after decoding. Missing, invalid,
or asset-catalog-only icons display a placeholder.

## Configuration

Read at startup from `$XDG_CONFIG_HOME/launcher/config` when XDG_CONFIG_HOME
is absolute, otherwise `$HOME/.config/launcher/config`. Copy `config/config`
there to customize:

```text
roundEdges: true
```

Set `false` for square window corners; `true` (default) uses a 16-pixel radius.
The format is one `key: value` per line with optional `#` comments, not full YAML.
Missing files use defaults; invalid settings report an error in the terminal
and use defaults. Restart after editing. Escape closes immediately even while
the search input is focused.


The launcher follows macOS light/dark appearance, including changes while open.
Defaults are [Catppuccin](https://catppuccin.com/palette/) Mocha for dark and Latte
for light; unavailable system appearance falls back to dark. Override any colors:

```text
dark.background: #1e1e2e
dark.foreground: #cdd6f4
dark.accent: #89b4fa
light.background: #eff1f5
light.foreground: #4c4f69
light.accent: #1e66f5
```

Colors must be unquoted `#RRGGBB` values (case insensitive). Background controls
the panel, foreground controls normal text, and accent controls selection and
focus. Input surfaces and hover states are derived by Iced from this palette.
Missing or invalid colors retain that appearance's default for that color; invalid
colors also log a warning. Unknown keys, duplicates, or malformed lines reject the
configuration and restore all defaults. Custom colors are used as supplied; choose
contrasting foreground/background colors. The complete example is `config/config`.

## Install as a macOS app

From this repository:

```sh
./scripts/install.sh
```

This builds a release binary and installs an ad-hoc-signed bundle at
`/Applications/Launcher.app`. It refuses to overwrite an existing application.
Move an old installation aside before updating. If /Applications is not writable,
use `./scripts/install.sh "$HOME/Applications"` and adjust the shortcut path below.
The build currently uses cached Cargo dependencies (`--offline`); run `cargo fetch`
first on a new machine. To only build the bundle, run `./scripts/package.sh`;
the result is `target/bundle/Launcher.app`. Signing is local, not notarization
for distribution.

### Option+Space with AeroSpace

Add this to your AeroSpace configuration (inside the existing binding table):

```toml
[mode.main.binding]
alt-space = 'exec-and-forget /usr/bin/open /Applications/Launcher.app'
```

Add this at the top level so the launcher floats rather than being tiled:

```toml
[[on-window-detected]]
if.app-id = 'io.github.0x0bsod.launcher'
run = 'layout floating'
```

This uses [AeroSpace's binding and window callback support](https://nikitabobko.github.io/AeroSpace/guide.html).
The launcher exits on Escape or successful launch, and the shortcut opens it again.
AeroSpace must be running; there is no launcher background process.

For this machine's Nix-managed AeroSpace configuration, apply the prepared source
patch instead of editing the generated `~/.aerospace.toml`:

```sh
git -C "$HOME/Projects/github.com/0x0bsod/nix" apply --check "$PWD/packaging/aerospace.nix.patch"
git -C "$HOME/Projects/github.com/0x0bsod/nix" apply "$PWD/packaging/aerospace.nix.patch"
```

Run your normal nix-darwin rebuild, then `aerospace reload-config`.
Test Option+Space, type a query, press Escape, and press Option+Space again.
