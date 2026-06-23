# OKLCH Dynamic Theming Engine

Snippy's UI requires themes that are expressive, dynamic, contrast-safe, and visually cohesive. To replace the hardcoded static colors, we introduced a dynamic theme engine based on the OKLCH color space.

## Key decisions

**OKLCH as the primary color space for theme generation.**
Unlike RGB or HSL, OKLCH is perceptually uniform. Its Lightness (L) channel matches human perception of brightness across all hues. This allows us to define background, foreground, widget states, and text colors mathematically (e.g. `bg_lightness = 0.08`, `text_lightness = 0.95`), guaranteeing consistent contrast ratios regardless of the selected accent hue.

**Gamut mapping via binary search on Chroma.**
Because OKLCH can represent colors outside the sRGB color gamut, converting OKLCH directly to RGB can lead to clamping and hue shifts. To solve this, our conversion routine dynamically scales the Chroma (C) down while preserving Lightness (L) and Hue (H) using a binary search (12 iterations) to find the maximum in-gamut chroma.

**Integrated neutrals (tinting gray palettes).**
Instead of using neutral gray backgrounds (which look sterile and disconnected), our panel, card, and widget backgrounds are slightly tinted with a customizable neutral hue (Neutral Hue) and intensity (Bg Chroma). This ties the visual palette together, making the UI feel cohesive.

**Interactive settings modal controls for Custom Dynamic theme.**
When the user selects the "Custom Dynamic" theme, the settings UI exposes native color pickers that dynamically map picked sRGB colors back into OKLCH space, offering a highly intuitive customization flow:
1. **Mode**: Dark, Light, or Custom. In Dark and Light modes, the background lightness is locked to contrast-safe presets.
2. **Primary Accent Color**: A single color picker replacing separate hue, chroma, and lightness sliders (automatically deriving and updating OKLCH parameters).
3. **Color Theory Options (Collapsible)**:
   - **Semantic Style**: Choose between "Classic" (adapted red/green/yellow for contrast-safe standard states) and "Harmonized" (completely derived from the chosen color harmony).
   - **Color Harmony**: Determines secondary and semantic color derivation (supporting Complementary, Split-Complementary, Analogous, Triadic, Tetradic, and Monochromatic).
   - **Background Tint** (Dark/Light modes) or **Background Color** (Custom mode): Pickers that allow customizing or decoupling the surface colors, automatically mapping back to background lightness, chroma, and neutral hue.
   - **Secondary Color**: A picker to preview and scale the chroma of the auxiliary color used for elements like the zoom slider active track and the AV Sync telemetry panel.

Changes are applied immediately in real time to the egui context.

**Refactoring hardcoded static colors to dynamic token references.**
To ensure visual consistency, we systematically replaced hardcoded RGBA values (e.g. static whites, dark blues, or generic grays) across the viewport background, empty state text, telemetry indicators, timeline elements, and notification banners with references to `ThemePalette`. This guarantees that all UI elements adapt correctly and remain fully legible in light, dark, and custom themes.

## Considered options (rejected)

- **RGB or HSL-based palette generation** — rejected because non-uniformity makes contrast guarantees extremely complex (e.g. blue and yellow have vastly different perceived brightness at the same lightness level in HSL).
- **Adding dependency on external color-space crate** — rejected to avoid unnecessary build-time overhead and dependency bloat. Implementing the OKLCH-to-sRGB transformation and gamut-mapping inline in `theme.rs` is lightweight, highly optimized, and self-contained.
