# LightningPoS POS enclosure

Two designs are provided:

* **`gen_v2.py`** — **new FLAT terminal** (posé à plat sur le comptoir).
* **`gen.py`** — previous wedge design (legacy).

Both produce `body.stl` and `battery_door.stl` next to the script.
Add `-- --preview` for renders (`preview.png`, `preview_front.png`,
`preview_iso.png`). Add `-- --section` for a cross-section render.

---

## v2 — Flat terminal

    blender --background --python gen_v2.py

### Envelope

66 (W) × 96 (D) × 24 (H) mm assembled (22 mm body + 2 mm door bottom).
Rounded corners, 5 mm plastic margin around the PCB.

### Layout

* **Top** — 320×480 portrait screen flush with the surface, surrounded by a
  plastic lip with honeycomb texture.
* **Inside** — ESP32-S3 PCB (56×86×8 mm) screwed down on 4 M2.5 bosses.
* **Under the PCB** — RC522 NFC module pocket in the bottom shell
  (60×40×3 mm, coil facing down through 1.5 mm plastic skin) and LiPo
  battery cavity (≈50×34×5 mm).
* **Back wall** — USB-C cable notch for flashing/charging.
* **Bottom** — snap-fit battery door with peripheral lip and retention ribs.

### Print orientation

* **body.stl** — bottom face down on the bed. Flat base, vertical walls,
  chamfered screen lip: no supports needed.
* **battery_door.stl** — flat, lip facing up. No supports.

### Assembly

1. Insert the **RC522** coil-side-down into the bottom pocket.
2. Drop the **LiPo battery** into the rear cavity.
3. Route wires through the cavity and plug into the ESP board.
4. Place the **ESP board** screen-up; align its 4 corner holes with the
   M2.5 bosses and screw it down.
5. Snap the **battery door** into the bottom opening.

### Key constants

| Constant | Effect |
|---|---|
| `LIP` | plastic margin around the PCB (drives outer W/D). |
| `H` | body height. Raise if you need more wiring room. |
| `SCREW_OFFSET` | hole-to-PCB-edge distance; **verify against your board**. |
| `NFC_SKIN` | plastic under the NFC coil (≤ 1.5 mm). |
| `CLEAR` | per-side clearance on component pockets. |

### DfAM check (automatic)

Every `gen_v2.py` run now prints a **DfAM analysis** per exported part right
after the bbox report — wall thickness (min / p05 / median), overhangs < 45°,
watertightness. It uses the Hermes skill `dfam-check`
(`~/.hermes/venvs/dfam` + `~/.hermes/skills/3d-printing/dfam-check`); a
missing tool degrades to a note, it never fails the build.

Standalone check of any STL:

    dfam-check.sh <file.stl> [--angle-limit 45] [--orientations]

**Open findings (2026-08-25):**

- **`battery_door.stl` — wall p05 = 1.0 mm** (limit FDM ≥ 1.2 mm supported /
  ≥ 1.6 mm unsupported). The door's thin walls should be thickened ~0.5 mm on
  the next design pass; print with 4 perimeters meanwhile.
- `body.stl` reports `watertight: false` — **expected**: it is an open shell
  (the top plate closes it). Not a defect. Reference: Blender
  `check_watertight.py` reports 0 non-manifold edges.
- `top_plate.stl` is watertight, single body — good.

---

## v1 — Wedge terminal (legacy)

    blender --background --python gen.py

### Envelope

92 (W) x 132 (D) x 95 (H) mm. Y=0 is the customer side, Z=0 the counter.

Side profile, front to back: 24 mm vertical nose -> NFC face raked 15 deg
from vertical (carries the RC522) -> brow crease -> display panel tilted
22 deg back -> top rear edge -> rear rake down to a 16 mm rear plinth.

## Print orientation

Both parts print **flat on the bed, as exported. No supports.**

* **body.stl** - bottom face down, exactly as the STL sits. Every
  downward-facing surface is >= 45 deg from horizontal: the internal void has
  a 45 deg gable roof, the rear rake is ~51 deg, the NFC face 75 deg.
  0.2 mm layers, 3-4 perimeters, 12-15% gyroid infill. Brim optional.
  The display-slot side walls are ~2.4 mm - do not drop below 3 perimeters.
* **battery_door.stl** - flat, bumps to the side. 0.2 mm, 4 perimeters,
  25% infill. The detent bumps need solid walls to snap without shearing.

The internal void is open to the battery bay, so there are no sealed
cavities and the slicer will not trap unreachable infill.

## Assembly

1. **RC522** goes in first, through the battery opening in the bottom.
   Feed it up into the void and forward into the front pocket, coil side
   against the 1.5 mm skin (the face with the contactless mark on the
   outside). Wires exit rearward through the 8 mm duct into the void.
2. **ESP board** slides into the display slot **from the rear**, screen up,
   USB-C edge last. It seats against the closed end at the brow; the panel
   slopes up toward the rear so it cannot fall out. The last 16 mm of the
   slot is 0.4 mm narrower and pinches the board.
   The USB-C port ends up at the slot mouth on the rear rake, with a
   14 x 6 mm cable relief notch - reflash without opening anything.
   **The screen ends up rotated 180 deg** relative to the customer; rotate
   in software (`setRotation`).
3. Route the RC522 and battery leads up the 9 mm vertical spine into the
   connector relief under the board's trailing end, then to the headers.
4. **Battery** drops into the bay from the bottom. Front and side ledges
   cap it.
5. **Door** presses in flush; four detent bumps snap into grooves in the bay
   walls. To remove, hook a fingernail into the notch at the rear edge.

## Constants to tweak

| Constant | Effect |
|---|---|
| `CLEAR` | master fit, per side, on every component cavity. 0.2 tight / **0.3 default** / 0.45 loose. Bump it first if anything binds. |
| `DETENT_PROUD` | door snap force. 0.55 default; 0.4 if the door is hard to seat, 0.7 if it rattles. |
| `DETENT_D` | groove depth; keep it above `DETENT_PROUD`. |
| `PINCH` / `PINCH_LEN` | board slot friction. Raise `PINCH` if the board is loose, drop to 0 for a free slide. |
| `NFC_SKIN` | plastic over the antenna. 1.5 max; drop to 1.0 for weaker cards. |
| `BEZEL_T` | lip over the screen border. Raising it recesses the screen deeper. |
| `WIN_W` / `WIN_L` | screen window. **Measure your panel's active area** and adjust - these are conservative (78 x 50). |
| `FILLET` | outer edge radius. 3.0 default; above ~4 the bevel starts clamping on the short profile segments. |
| `NOSE_H` | front nose height. See the note below before changing. |

### Height budget

`NOSE_H + NFC_RISE + DISP_RISE = H`, and `NFC_RISE` is *derived*. The two
component fits set the floor:

* RC522 needs `NFC_SLOPE >= 40.6 + 2 * NFC_MARGIN`, i.e. ~48.3 mm of rise
* ESP board needs `DISP_SLOPE >= 56.6 + PCB_END_WALL`, i.e. ~22.7 mm of rise

which leaves 24 mm for the nose at H=95. Raising `NOSE_H` to 35 requires
raising `H` to ~106 or the RC522 stops fitting - the script prints both
slope lengths against their requirements on every run, so a bad combination
is visible immediately.

Widening past 92 mm is the cheapest way to relax things: the display slot
walls are the tightest feature in the model at ~2.4 mm.