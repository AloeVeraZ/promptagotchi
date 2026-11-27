# Making your own pet

A pet is a folder with a `pet.json` in it. Put the folder in `~/.promptagotchi/pets/`, then pick it from the pet's right-click menu (**Pet → Reload pets** if it doesn't show up yet).

There are four ways to make one, from no code at all to bring-your-own-art:

1. [Pet Studio](#1-pet-studio-no-code): draw it in the app.
2. [Text pixels](#2-text-pixels): type the frames into `pet.json`.
3. [Images](#3-images-gifs-frames-or-a-sprite-sheet): one GIF per state, image frames, or a sprite sheet.
4. [Codex / Petdex pets](#4-codex-and-petdex-pets): already have one? It just works.

## The states

| State | When it plays | Falls back to |
| --- | --- | --- |
| `idle` | Nothing to do. **Required.** | — |
| `working` | Claude is busy in any session | `idle` |
| `question` | A session needs you | `wave` → `idle` |
| `done` | A session finished and is ready for you | `wave` → `idle` |
| `error` | A turn stopped with an error | `question` → `idle` |
| `sleep` | Nothing has happened for 5 minutes | `idle` |
| `drag` | You're carrying the pet | `jump` → `idle` |
| `dragLeft` / `dragRight` | Carrying it left / right (optional; one is mirrored to make the other) | `drag` |
| `wave` | Plays **once** when you hover | `idle` |
| `jump` | Plays **once** when a question arrives | `idle` |
| `look`, `stretch` | Idle fidgets, played **once** now and then while the pet stands around | skipped |
| `hit` | Plays **once** when you crack the whip across it | `jump` → `idle` |

So `idle` + `working` is already a complete pet. Add `question` next; it's the one you'll see most.

Bubbles (**?**, **✓**, **z z**) are drawn by the app, so don't put them in your art. The app finds the top of your pet's head in the first `idle` frame and sits the bubble right on it.

## 1. Pet Studio (no code)

Open Promptagotchi and pick the **Editor** tab (or right-click the pet → **Make or install pets → Pet Editor…**).

- **New blank pet** or **Copy this pet** to start from Pip or Mochi.
- Pick a state along the top. A state you haven't drawn yet starts as a copy of your idle frame.
- Tools: pencil <kbd>P</kbd>, eraser <kbd>E</kbd>, fill <kbd>F</kbd>, color picker <kbd>I</kbd> (or <kbd>⌥</kbd>-click). Right-drag erases.
- <kbd>⌘Z</kbd> / <kbd>⇧⌘Z</kbd> undo and redo. <kbd>←</kbd> <kbd>→</kbd> step through frames. **Onion skin** shows the previous frame faintly.
- Click a color swatch again to change it, <kbd>⌥</kbd>-click to remove it (if unused), and **＋** to add one.
- Each frame has its own duration (**Frame ms**). Turn off **Loop** for one-shot states.
- **Save & use** writes `~/.promptagotchi/pets/<id>/pet.json` and puts your pet on the desktop.

Studio saves the same text-pixel format described next, so you can switch between Studio and a text editor freely.

## 2. Text pixels

Each frame is a list of strings, one string per row and one character per pixel. The `palette` maps characters to colors. `.` (or a space) is transparent.

Here's a complete (tiny) pet:

```json
{
  "id": "beep",
  "name": "Beep",
  "author": "you",
  "description": "A very small robot.",
  "frameWidth": 8,
  "frameHeight": 8,
  "palette": {
    ".": "transparent",
    "g": "#9AA3AD",
    "e": "#6FE3FF",
    "d": "#5B6470"
  },
  "animations": {
    "idle": {
      "durations": [900, 150],
      "frames": [
        ["...g....", "..ggg...", ".gegegg.", ".ggggg..", ".ggggg..", "..d.d...", "..d.d...", "........"],
        ["...g....", "..ggg...", ".ggggg..", ".ggggg..", ".ggggg..", "..d.d...", "..d.d...", "........"]
      ]
    },
    "working": {
      "fps": 8,
      "frames": [
        ["...g....", "..ggg...", ".gegegg.", ".ggggg..", ".ggggg..", "..d.d...", "..d.d...", "........"],
        ["........", "...g....", "..ggg...", ".gegegg.", ".ggggg..", ".ggggg..", "..d.d...", "........"]
      ]
    }
  }
}
```

Tips:

- **32×28** is a good size. Pip is 32×28 and Mochi is 16×14. The app scales pixel art by whole numbers, so it stays crisp.
- Keep the feet on the **same row** in every state, so the pet doesn't hop when it switches state.
- Leave a few empty rows at the top for jumps.
- Colors can be `#rgb`, `#rrggbb`, `#rrggbbaa` or `rgba(…)`. Pip's soft ground shadow is `rgba(0,0,0,0.22)`.
- Generated art is fine too. Pip is drawn by [`scripts/build-pip.mjs`](../scripts/build-pip.mjs) with a few `rect()` calls.

## 3. Images: GIFs, frames, or a sprite sheet

Set `"pixelArt": true` for crisp scaling of pixel art, or `false` for smooth scaling of illustrations.

**One animated GIF / APNG / WebP per state** (the easiest if you already have GIFs):

```json
{
  "id": "frog",
  "name": "Frog",
  "frameWidth": 64,
  "frameHeight": 64,
  "pixelArt": true,
  "animations": {
    "idle":     { "image": "idle.gif" },
    "working":  { "image": "typing.gif" },
    "question": { "image": "hey.gif" },
    "done":     { "image": "yay.gif" }
  }
}
```

`frameWidth` / `frameHeight` should match the GIF's size; they're used for scaling.

**Separate image files:**

```json
"working": { "frames": ["work-1.png", "work-2.png", "work-3.png"], "fps": 8 }
```

**A sprite sheet:** frames sit side by side in a row. Pick the row, the number of frames, and optionally the starting column:

```json
"frameWidth": 48,
"frameHeight": 48,
"animations": {
  "idle":    { "sheet": { "image": "sheet.png", "row": 0, "count": 4 }, "fps": 4 },
  "working": { "sheet": { "image": "sheet.png", "row": 1, "count": 6 }, "durations": [90, 90, 90, 90, 90, 200] }
}
```

All paths are relative to the pet's folder and can't point outside it.

## 4. Codex and Petdex pets

Promptagotchi reads the Codex pet format as-is: a `pet.json` with `spritesheetPath`, and an 8×9 atlas of 192×208 cells. Pets in `~/.codex/pets/` and `~/.petdex/pets/` appear under **Pet → From Codex / From Petdex** automatically. Their rows map like this:

| Atlas row | Promptagotchi state |
| --- | --- |
| 0 idle | `idle` |
| 1 running-right / 2 running-left | `dragRight` / `dragLeft` |
| 3 waving | `wave` |
| 4 jumping | `jump` |
| 5 failed | `error` |
| 6 waiting | `question` |
| 7 running | `working` |
| 8 review | `done` |

## Reference: `pet.json`

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | yes | Folder-safe name: letters, numbers, `-`, `_` |
| `name` | yes | Shown in menus |
| `author`, `description` | no | Shown in Pet Studio |
| `frameWidth`, `frameHeight` | yes | Size of one frame in pixels |
| `pixelArt` | no | `true` (default) = crisp nearest-neighbour scaling |
| `palette` | text pixels | Character → color |
| `fps` | no | Default speed for every animation |
| `animations` | yes | State → animation (below) |

An animation has **one** source:

- `frames`: either a list of text-pixel frames, or a list of image paths.
- `sheet`: `{ "image", "row", "count", "col"? }`
- `image`: an animated GIF / APNG / WebP.

It can also set its timing and looping:

- `fps`, or `durations` (milliseconds per frame).
- `loop`: defaults to `true`, except for `wave` and `jump`.

## Check and share

```bash
node scripts/preview-pet.mjs ~/.promptagotchi/pets/beep     # renders preview.png with every frame
```

Pet Studio and the app both show warnings for problems such as a row that's the wrong width, a missing palette color, or an unknown state.

To share a pet, zip its folder. Friends can install it with **Pet → Install a pet from a folder…**. Want it bundled with the app? Open a pull request that adds it under `pets/`.
