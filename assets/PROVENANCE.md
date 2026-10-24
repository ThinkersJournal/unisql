# Provenance of the files in `assets/`

Read 2026-10-24 at `origin/main` `810f7e5d`, from the files' bytes and from `git log`. The licence
reading for the vector logo is in `docs/LICENCES.md` ("The logo"); this file adds the per-file record.

| File | Bytes | SHA-256 | First added | Origin |
|---|---|---|---|---|
| `Unicycle.svg` | 14,157 | `f75c386dce6dd761a52faf290f2790f486f2a1a5ac67023e24dd556d1f69861d` | `987f61d`, 2026-10-12, committed by the project owner | **Verified**: byte-identical to Openclipart "Unicycle" by GDJ (<https://openclipart.org/detail/221146/unicycle>) |
| `Unicycle-Large.png` | 106,147 | `f70739ebd77d14407b9d42f580802f43adb269dc696607e56b0994331dfd8a7c` | `987f61d`, 2026-10-12 | **Not established** (see below) |
| `Unicycle-Medium.png` | 36,154 | `0ebfc5b1a044520b86100030592dbb509dfa5f24ed62964d724d7eb14e4dbd32` | `987f61d`, 2026-10-12 | **Not established** |
| `Unicycle-Small.png` | 15,490 | `436952f91e934f02d2c7b74fc0bd2032a829e9080d6a392526ef269968bfea10` | `987f61d`, 2026-10-12 | **Not established** |

## What the PNGs say about themselves

Read with a PNG chunk parser, read-only. Each PNG carries only the standard chunks (`IHDR`, `gAMA`,
`cHRM`, `pHYs`, `tIME`, `bKGD`, two `tEXt`, `IEND`):

| PNG | Pixels | `tIME` (UTC) | `tEXt` `date:create` / `date:modify` |
|---|---|---|---|
| Large | 807 x 1406 | 2020-05-05 09:58:25 | 2020-05-05T09:58:24+00:00 (both) |
| Medium | 323 x 563 | 2019-09-03 00:18:31 | 2019-09-03T00:18:31+00:00 (both) |
| Small | 162 x 283 | 2020-01-17 16:00:11 | 2020-01-17T16:00:11+00:00 (both) |

- The `date:create` / `date:modify` pair is the form an ImageMagick-style PNG writer emits. That is a
  property of the encoder, not a statement of who made the picture or from what.
- **No author, copyright, software, title, licence, XMP, EXIF, ICC or C2PA / content-credentials data**
  is present in any of the three (no `iTXt`, `zTXt`, `eXIf`, `iCCP`, `caBX` chunk; none of the byte
  markers `c2pa`, `jumb`, `xmp`, `Adobe`, `Software` occur). So the files do not record a maker or a tool.
- The timestamps are 2019-2020: before this repository (first commit 2026-10-12), so the files were made
  elsewhere and copied in. The git history has one commit that adds them, with no message about them.
- `docs/LICENCES.md` already notes that the PNGs look like renders of the SVG but were not compared with
  anything. That is still so: no SVG rasteriser was available when this was read, so the pixels were not
  compared with the SVG either. The aspect ratio of the PNGs (about 0.574) differs from the SVG's page
  (`viewBox` 595.28 x 841.89, about 0.707), which fits a render cropped to the drawing but proves nothing.

## Licence

- `Unicycle.svg`: public domain per Openclipart's statement (`docs/LICENCES.md`). Credit: **Unicycle, by GDJ, Openclipart.**
- The three PNGs: **no licence of their own is recorded.** If they are renders of the SVG they follow the
  SVG's; that is unproven. Until the maker is known they are covered by neither the project's
  `MIT OR Apache-2.0` (which is for the code) nor any asset licence on record. They are used only as the
  logo image in `docs/DESIGN.md`, and are not part of any published crate.

## Open question (for the project owner)

Who made `Unicycle-{Large,Medium,Small}.png`, with what tool, and from what source: renders of GDJ's SVG
(by whom and with what), or something else? Once answered, replace the "Not established" cells with the
answer, and with a licence line for the PNGs.
