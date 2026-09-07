# Todo app icon

The icon uses the application's Solarized palette: deep teal for the tile,
warm parchment for task lines, and olive-lime for the completion mark. The
three shortening lines and forward-rising check communicate progress while
remaining readable at Dock and Cmd-Tab sizes.

`app-icon-concept.png` is the image-generation output. The production master
is `../src-tauri/icons/icon-source.png`; the remaining files in that directory
are generated desktop bundle sizes. `../src-tauri/icons/icon.svg` is the
editable vector source.

## Generation prompt

```text
Use case: logo-brand
Asset type: 1024x1024 master artwork for a native macOS productivity app icon shown in Applications, Launchpad, Cmd-Tab, and Dock
Primary request: Create a distinctive icon for a keyboard-first personal todo app. The central mark is a bold, geometric checkmark integrated with three short horizontal task strokes, reading instantly as completion and momentum. No letters and no words.
Scene/backdrop: a single icon tile only; artwork fills the square canvas, with the emblem centered and generous optical breathing room inside the macOS icon safe area
Style/medium: exceptionally clean vector-friendly flat design with subtle material depth, crisp silhouette, restrained premium macOS aesthetic, legible at 16px
Color palette: Solarized-inspired deep teal #073642 and near-black teal #002B36, warm parchment #FDF6E3, signature olive-lime #859900; olive is the memorable accent
Composition/framing: perfectly square 1:1 front-facing icon master, balanced and centered
Constraints: opaque background extending to every canvas edge; simple large shapes; strong contrast; no tiny details; no text; no numbers; no watermark; no device mockup; no screenshot; no white margin; do not show the icon floating in a scene
Avoid: generic clipboard, calendar page, pencil, photorealism, glossy skeuomorphism, excessive gradients, an outer pre-rounded square silhouette or baked-in transparent corners
```

Generated with the built-in image generation tool, then fitted to the macOS
icon safe area and exported with the Tauri icon pipeline.
