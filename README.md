# Compare

A small desktop viewer for before-and-after screenshots. Built with Rust and GPUI.

Drop in as many photos as you want. Pick one for before and one for after, then name the pair. Creating a pair stays in the library. Open a pair when you want the full comparison. Before stays on the left of the line. After stays on the right. The line can sit on either edge.

Name a group and check the pairs that belong to it. A pair can sit in more than one group. The library sidebar filters the list. In the viewer, Groups chooses which comparisons appear, and Show all pairs clears that filter. Combined groups list each pair once.

In the viewer, the current pair fills the window. Up and down move through the pairs in that filter. The previous pair shows along the top and the next pair shows along the bottom. The first pair has nothing above it. The last pair has nothing below it.

Auto pair matches numbered filenames such as `before_1` with `after_1`, including `10`, and keeps surrounding text like `home_` as part of the match. Notes and numbered pins stay on the pair. Overlay blends both shots on one canvas; Wipe keeps each pair's divider. Paste puts a clipboard image into the selected Before or After slot, and Save keeps that image in the workspace. Drag a pair to reorder the list the viewer uses. Search jumps to a pair by its name or filename.

Everything stays on this Mac, in `~/Library/Application Support/Compare`. Compare copies each import into that folder and writes the workspace beside the copies. There is no account and no server. Quitting and reopening restores the uploads, pairs, groups, notes, order, and the comparison you last had open.

## Run

```shell
cargo run
```

The first build compiles GPUI and takes a few minutes.

## Keys

| Key | Action |
| --- | --- |
| Drop or ⌘O | Add images |
| Create pair | Choose before, then after, then name the pair |
| Enter | Open the selected pair, create the pair you are naming, or create the group you are naming. Creating stays in the library |
| ↑ ↓ | Previous or next pair in the current filter |
| Drag the line, or ← → | Move the mask, including both edges |
| Shift ← / Shift → | Jump the mask to the left or right edge |
| Fit | Fit the screenshot, or fill the frame |
| Groups | Choose which groups appear in the viewer. Show all pairs clears the filter |
| ⌘F | Search pairs and jump to one. Up and down move through results while search is open |
| ⌘V | Paste a clipboard image into the selected Before or After slot |
| Auto pair | Preview numbered filename matches, then create the valid pairs |
| Notes | Pair notes and numbered pins. Add pin marks a point or a rectangle |
| Overlay | Blend Before into After. Returning to Wipe restores the divider |
| Esc | Close search, auto pair, notes, or a menu first. Then leave the viewer, or cancel pairing, editing, or a new group |
| Delete | Unlink the selected pair |

JPEG, PNG, WebP, GIF, BMP, and TIFF open directly. HEIC and HEIF are converted with `sips` on macOS.
