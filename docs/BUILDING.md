# Building UsageBar locally

This fork keeps the upstream UsageBar name, application identifier, provider
catalog, and updater endpoint. Local and CI builds do not create a Git tag or a
GitHub Release.

## Prerequisites

- Node.js 22 or newer
- The stable Rust toolchain
- The platform prerequisites from the
  [Tauri 2 documentation](https://v2.tauri.app/start/prerequisites/)

Install JavaScript dependencies and run the shared checks first:

```sh
npm ci
npm test
cargo test --manifest-path src-tauri/Cargo.toml
```

## Windows

Build both supported installer formats from PowerShell:

```powershell
npm run build -- --bundles msi,nsis --no-sign
```

The unsigned local packages are written below
`src-tauri/target/release/bundle/msi` and
`src-tauri/target/release/bundle/nsis`. WebView2 is downloaded by the installer
when it is not already present.

The NSIS package is the primary interactive Windows installer. It includes
English, Russian, and Simplified Chinese, an install-time **Start with Windows**
choice, directory and Start Menu pages, and explicit finish-page choices for a
desktop shortcut and launching the app. Passive updater installs preserve the
existing autostart choice.

## Linux

Install the WebKitGTK, tray, SVG, and packaging dependencies for the target
distribution. On Ubuntu 22.04 the CI lane uses:

```sh
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
npm run build -- --bundles deb,appimage --no-sign
```

Packages are written below `src-tauri/target/release/bundle/deb` and
`src-tauri/target/release/bundle/appimage`.

## macOS

The application supports macOS 12 and newer. Build each architecture natively:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm run build -- --target aarch64-apple-darwin --bundles dmg,app --no-sign
npm run build -- --target x86_64-apple-darwin --bundles dmg,app --no-sign
```

These development packages are ad-hoc signed and not notarized. Their DMG and
app bundles are written under the matching target's `release/bundle` directory.

## Provider coverage

The complete catalog remains available: Codex, Cursor, Grok, GLM, ZCode,
Claude, Copilot, Gemini, and Antigravity. Provider credentials stay in their
existing native application files, environment variables, or supported local
configuration; build jobs do not require or upload credentials.

For a credential-aware smoke test, open **Providers…**, select the providers
available on that machine, and confirm that each card either shows quota data
or its localized sign-in/configuration state. No provider should disappear
because another provider lacks credentials.

The workflow in `.github/workflows/ci.yml` performs the same native builds on
pull requests or when manually dispatched. It uploads workflow artifacts only;
it does not publish a release.

The tag-driven `.github/workflows/release.yml` uses the upstream Tauri signing
secret when it is available. A fork without that secret automatically adds
`--no-sign`, so its own `v*` tags can still publish unsigned packages on the
fork's GitHub Releases page.
