# Settings Architecture

Snippy's Settings system stores user-controllable values as TOML in the OS config directory (`~/.config/snippy/settings.toml` on macOS/Linux, `%APPDATA%\snippy\settings.toml` on Windows), loaded once at startup and owned by `MainApp`.

## Key decisions

**TOML over platform-native stores or JSON.** TOML is idiomatic Rust (used by Cargo), human-readable, and hand-editable without knowledge of Rust. Platform-native stores (NSUserDefaults, Windows Registry) would sacrifice cross-platform inspectability and make it harder for users to reset or share their configuration.

**Advanced settings are file-only; there is no Advanced UI panel.** The Settings modal surfaces only common settings. Power-user knobs (thumbnail thread count, LRU cache size, presenter lookahead depth, per-pipeline HW accel overrides, zoom formula bounds and behaviour) exist only as TOML fields. The modal provides an "Open config file in editor" button with the full path displayed, making the file discoverable without cluttering the UI. This keeps the settings surface small and forces advanced options to be self-documenting via TOML comments.

**First launch generates a fully-commented default file.** Since advanced settings have no UI, they are invisible unless the file exists. Writing it on first launch — with a comment above every field explaining its purpose and valid values — matches the approach used by terminal apps like Alacritty and Helix.

**Corrupt file: rename to `.bak`, regenerate defaults, push a Notification.** A wholly invalid TOML file is renamed to `settings.toml.bak` (preserving the user's edits for recovery), a fresh default file is written, and a Notification is pushed to the UI. Individual missing fields (e.g. from an older version of the file) fall back to their coded defaults via `#[serde(default)]` without triggering a full reset.

**No hot-reload; settings are applied at startup only.** Reloading on focus-regain or via a file watcher would require a background thread or polling and complicates the ownership model. Settings that require a restart to take effect (HW accel, thread counts) show a "restart required" indicator in the UI when changed. Settings that are safe to apply live (theme, default zoom) update immediately in-process.

**Settings are written to disk on Settings modal close.** Writing on every keystroke causes unnecessary disk churn; writing only on app exit risks losing changes if the app is force-quit. Modal close is the natural commit point and is always preceded by any live in-process application of the change.

**Hardware acceleration is exposed as one UI toggle but two independent TOML fields.** `playback_hw_accel` and `thumbnails_hw_accel` are separate pipelines and may legitimately need independent control (e.g. a user experiencing GPU contention from thumbnail decoding). The UI checkbox writes both simultaneously for the common case; the TOML fields can be diverged by hand for fine-grained control.

## Considered options (rejected)

- **Hot-reload on focus regain** — avoided to keep Settings ownership simple and because the benefit is low (Settings changes are rare).
- **Per-field reset buttons** — avoided; deleting the `.toml` file and restarting is a sufficient and already-discoverable reset path via the "Open config file" button.
- **String unit suffixes in TOML** (`"30s"`, `"1 frame"`) for Zoom Interval bounds — rejected in favour of explicit `value + unit` field pairs (`max_zoom_value = 30`, `max_zoom_unit = "seconds"`) to avoid a custom parser and keep TOML natively validatable.
