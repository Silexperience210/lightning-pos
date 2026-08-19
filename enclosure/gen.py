#!/usr/bin/env python3
"""
LightningPoS POS terminal enclosure - parametric Blender generator.

Run:  blender --background --python gen.py
      blender --background --python gen.py -- --preview   (also renders preview.png)

Outputs (next to this script):
    body.stl           main wedge shell
    battery_door.stl   snap-in bottom door

All dimensions in millimetres, 1 Blender unit = 1 mm.
Everything below the "DERIVED" banner is computed - tweak only the constants.
"""

import math
import os
import sys

import bmesh
import bpy
from mathutils import Vector

# =====================================================================
#  CONSTANTS  -- tweak these
# =====================================================================

OUT_DIR = os.path.dirname(os.path.abspath(__file__))

# ---- global fit ------------------------------------------------------
CLEAR = 0.3           # FDM clearance, PER SIDE, on every component cavity
                      #   bigger = looser. 0.2 tight, 0.3 default, 0.45 loose

# ---- outer envelope --------------------------------------------------
W = 72.0              # width  (X)  (portrait: driven by the 65mm battery, not the display)
D = 132.0             # depth  (Y)  0 = front / customer side
H = 60.0              # height (Z)  0 = counter top

NOSE_H = 8.0          # height of the vertical front plinth (front "nose")
REAR_PLINTH_H = 14.0  # height of the vertical rear plinth
NFC_TILT = 30.0       # NFC face angle, degrees FROM VERTICAL (near-vertical front for tapping)
DISP_TILT = 15.0      # display panel angle, degrees FROM HORIZONTAL
DISP_RISE = 23.4      # vertical rise consumed by the display panel (86mm board @ 15 deg)

FILLET = 6.0          # outer edge fillet radius (softer silhouette)
FILLET_SEG = 8        # fillet smoothness
FILLET_ANGLE = 12.0   # only edges sharper than this get filleted
BOT_CHAMFER = 0.8     # 45 deg chamfer on the two bottom profile corners

WALL = 2.5            # generic wall thickness

# ---- 1. ESP32 board JC3248W535 (3.5" 320x480 touch) ------------------
PCB_W = 56.0          # across the terminal width (X)  -- PORTRAIT: short side = width
PCB_L = 86.0          # along the display slope       -- PORTRAIT: long side = height
PCB_T = 8.0           # total thickness incl. module + screen
BEZEL_T = 1.6         # plastic lip covering the screen border
WIN_W = 49.0          # screen window opening, across width  (portrait active area ~49mm)
WIN_L = 73.0          # screen window opening, along slope   (portrait active area ~73.4mm)
GLASS_W = 62.0        # full-face "glass" recess, across width
GLASS_L = 88.0        # full-face "glass" recess, along slope
GLASS_DEPTH = 0.8     # glass recess depth
PCB_END_WALL = 3.5    # closed (downhill) end of the board slot
PINCH = 0.4           # total width interference over the last PINCH_LEN mm
PINCH_LEN = 16.0      #   -> friction retention of the board in its slot

# ---- 2. RC522 NFC reader --------------------------------------------
NFC_W = 60.0          # across the terminal width (X)
NFC_L = 40.0          # along the NFC face slope
NFC_T = 3.0
NFC_SKIN = 1.5        # plastic in front of the antenna coil (<= 1.5 mm)
NFC_BACK_WALL = 2.2   # wall behind the module
NFC_MARGIN = 1.0      # free slope above/below the module on the NFC face

# ---- 3. Battery 126 x 65 x 10 ---------------------------------------
BAT_CAV_L = 127.0     # cavity along Y  (spec)
BAT_CAV_W = 66.0      # cavity along X  (spec)
BAT_CAV_T = 11.0      # cavity along Z  (spec)
DOOR_T = 2.4          # battery door thickness
DOOR_CLEAR = 0.3      # door clearance per side
DETENT_D = 0.7        # depth of the detent groove in the bay wall
DETENT_PROUD = 0.55   # how far the door's bump stands proud
DETENT_LEN = 15.0     # groove length (4 grooves total)
DETENT_Y = (35.0, 97.0)   # groove centres along Y

# ---- internal void / wiring -----------------------------------------
VOID_HW = 31.0        # void half width (X)
VOID_SIDE_H = 6.6     # straight side height of the void above the battery bay
VOID_ROOF = 45.0      # void roof angle from horizontal (>=45 = no supports)
FRONT_INNER = 7.0     # material thickness behind the whole front face
DISP_INNER = 11.5     # material thickness behind the display panel
SPINE_R = 6.0         # vertical wire duct: void -> board connector relief (12mm dia)
SPINE_Y = 118.0
NFC_DUCT_R = 6.0      # wire duct: NFC pocket -> void (12mm dia for 7 wires)
NFC_DUCT_U = 16.0     # position of that duct along the NFC face

# ---- USB-C access ----------------------------------------------------
USB_W = 14.0          # cable relief notch width  (opening ~14 x 6)
USB_H = 6.0
USB_Z = 36.0          # bottom of the notch

# ---- contactless mark ------------------------------------------------
MARK_R = 17.0         # recessed circle radius
MARK_DEPTH = 0.4      # circle recess depth
ARC_DEPTH = 0.8       # arc engraving depth (total, from the outer face)
ARC_RADII = (5.5, 9.5, 13.5)
ARC_T = 1.6           # arc stroke thickness
ARC_SPAN = 58.0       # half-span of each arc, degrees

# ---- rubber feet -----------------------------------------------------
FOOT_R = 4.0
FOOT_DEPTH = 0.6
FOOT_X = 38.0
FOOT_Y = (15.0, 117.0)

# =====================================================================
#  DERIVED
# =====================================================================

_nfc_a = math.radians(90.0 - NFC_TILT)     # NFC face up-slope angle from +Y
_dis_a = math.radians(DISP_TILT)           # display up-slope angle from +Y

NFC_RISE = H - NOSE_H              # NFC face rises from the nose to the brow (top)
NFC_RUN = NFC_RISE * math.tan(math.radians(NFC_TILT))
NFC_SLOPE = NFC_RISE / math.cos(math.radians(NFC_TILT))
DISP_RUN = DISP_RISE / math.tan(_dis_a)
DISP_SLOPE = DISP_RISE / math.sin(_dis_a)

BROW = (NFC_RUN, NOSE_H + NFC_RISE)        # NFC face / display panel crease
TOPR = (NFC_RUN + DISP_RUN, H - DISP_RISE)  # rear edge, LOWER -> screen faces the vendor

# outer silhouette in the Y-Z plane, clockwise
PROFILE = [
    (BOT_CHAMFER, 0.0),
    (0.0, BOT_CHAMFER),
    (0.0, NOSE_H),
    BROW,
    TOPR,
    (D, REAR_PLINTH_H),
    (D, BOT_CHAMFER),
    (D - BOT_CHAMFER, 0.0),
]

# component cavities (with clearance)
PKT_W = PCB_W + 2 * CLEAR
PKT_L = PCB_L + 2 * CLEAR
PKT_T = PCB_T + CLEAR

NFC_PW = NFC_W + 2 * CLEAR
NFC_PL = NFC_L + 2 * CLEAR
NFC_PT = NFC_T + CLEAR

BAY_Y0 = (D - BAT_CAV_L) / 2.0
BAY_Y1 = BAY_Y0 + BAT_CAV_L
BAY_HW = BAT_CAV_W / 2.0
BAY_Z0 = DOOR_T
BAY_Z1 = BAY_Z0 + BAT_CAV_T

VOID_Z0 = BAY_Z1
VOID_APEX = min(VOID_Z0 + VOID_SIDE_H + VOID_HW * math.tan(math.radians(VOID_ROOF)),
                H - DISP_INNER - 1.0)

# display slot, in panel coordinates (u = along slope from BROW, w = into panel)
PKT_U0 = PCB_END_WALL
PKT_U1 = DISP_SLOPE + 4.0          # runs past the top edge -> open mouth
PKT_W0 = -BEZEL_T
PKT_W1 = -(BEZEL_T + PKT_T)
BOARD_U1 = PKT_U0 + PKT_L          # where the board's trailing edge lands
WIN_U0 = PKT_U0 + 4.0
WIN_U1 = WIN_U0 + WIN_L

# NFC pocket, in NFC-face coordinates (u = along slope from the nose top)
NFCP_U0 = NFC_MARGIN
NFCP_U1 = NFCP_U0 + NFC_PL
MARK_U = (NFCP_U0 + NFCP_U1) / 2.0


# =====================================================================
#  MESH HELPERS
# =====================================================================

def _link(name, verts, faces):
    me = bpy.data.meshes.new(name)
    me.from_pydata(verts, [], faces)
    me.validate()
    ob = bpy.data.objects.new(name, me)
    bpy.context.collection.objects.link(ob)
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(me)
    bm.free()
    return ob


def prism(name, poly, axis, a0, a1):
    """Extrude a 2D polygon into a solid.
    axis='X': poly is [(y, z), ...]   axis='Y': poly is [(x, z), ...]"""
    n = len(poly)
    verts = []
    for a in (a0, a1):
        for p in poly:
            verts.append((a, p[0], p[1]) if axis == 'X' else (p[0], a, p[1]))
    faces = [list(range(n))[::-1], list(range(n, 2 * n))]
    for i in range(n):
        j = (i + 1) % n
        faces.append([i, j, n + j, n + i])
    return _link(name, verts, faces)


def box(name, x0, x1, y0, y1, z0, z1):
    return prism(name, [(y0, z0), (y1, z0), (y1, z1), (y0, z1)], 'X', x0, x1)


def circle_poly(cx, cy, r, seg=72):
    return [(cx + r * math.cos(2 * math.pi * i / seg),
             cy + r * math.sin(2 * math.pi * i / seg)) for i in range(seg)]


def cyl(name, r, x, y, z0, z1, seg=64):
    poly = [(x + r * math.cos(2 * math.pi * i / seg),
             y + r * math.sin(2 * math.pi * i / seg)) for i in range(seg)]
    n = seg
    verts = [(p[0], p[1], z0) for p in poly] + [(p[0], p[1], z1) for p in poly]
    faces = [list(range(n))[::-1], list(range(n, 2 * n))]
    for i in range(n):
        j = (i + 1) % n
        faces.append([i, j, n + j, n + i])
    return _link(name, verts, faces)


def panel_prism(name, poly_ux, w0, w1, origin, ang):
    """Solid built on a tilted panel.
    poly_ux : polygon in (u = along slope, x = across width)
    w       : along the panel's outward normal (negative = into the body)
    origin  : (y, z) of the panel's u=0, w=0 point
    ang     : up-slope angle from +Y, radians"""
    ca, sa = math.cos(ang), math.sin(ang)
    oy, oz = origin

    def P(u, x, w):
        return (x, oy + u * ca - w * sa, oz + u * sa + w * ca)

    n = len(poly_ux)
    verts = [P(u, x, w0) for (u, x) in poly_ux] + [P(u, x, w1) for (u, x) in poly_ux]
    faces = [list(range(n))[::-1], list(range(n, 2 * n))]
    for i in range(n):
        j = (i + 1) % n
        faces.append([i, j, n + j, n + i])
    return _link(name, verts, faces)


def panel_rect(name, u0, u1, hw, w0, w1, origin, ang):
    return panel_prism(name, [(u0, -hw), (u1, -hw), (u1, hw), (u0, hw)],
                       w0, w1, origin, ang)


def arc_poly(cu, cx, r_in, r_out, a0, a1, seg=48):
    """Annular sector outline in (u, x)."""
    pts = []
    for i in range(seg + 1):
        a = math.radians(a0 + (a1 - a0) * i / seg)
        pts.append((cu + r_out * math.cos(a), cx + r_out * math.sin(a)))
    for i in range(seg, -1, -1):
        a = math.radians(a0 + (a1 - a0) * i / seg)
        pts.append((cu + r_in * math.cos(a), cx + r_in * math.sin(a)))
    return pts


# ---- 2D line utilities (for the offset inner shell) ------------------

def seg_line(p, q, offset):
    """Line through p->q pushed `offset` toward the polygon interior.
    PROFILE is wound clockwise, so the inward normal is (dz, -dy)."""
    d = (q[0] - p[0], q[1] - p[1])
    L = math.hypot(*d)
    d = (d[0] / L, d[1] / L)
    nrm = (d[1], -d[0])
    return ((p[0] + nrm[0] * offset, p[1] + nrm[1] * offset), d)


def line_isect(l1, l2):
    (p, d), (q, e) = l1, l2
    den = d[0] * e[1] - d[1] * e[0]
    t = ((q[0] - p[0]) * e[1] - (q[1] - p[1]) * e[0]) / den
    return (p[0] + d[0] * t, p[1] + d[1] * t)


# ---- modifier helpers ------------------------------------------------

def _apply(ob, mod):
    bpy.context.view_layer.objects.active = ob
    bpy.ops.object.modifier_apply(modifier=mod.name)


def boolean(target, cutter, op='DIFFERENCE'):
    m = target.modifiers.new(f"bool_{cutter.name}", 'BOOLEAN')
    m.operation = op
    m.object = cutter
    m.solver = 'EXACT'
    _apply(target, m)
    bpy.data.objects.remove(cutter, do_unlink=True)
    return target


def fillet(ob, width, segments, angle_deg):
    m = ob.modifiers.new("bevel", 'BEVEL')
    m.width = width
    m.segments = segments
    m.limit_method = 'ANGLE'
    m.angle_limit = math.radians(angle_deg)
    m.miter_outer = 'MITER_ARC'
    try:
        m.clamp_overlap = True
    except AttributeError:
        pass
    _apply(ob, m)
    return ob


def bbox(ob):
    vs = [ob.matrix_world @ v.co for v in ob.data.vertices]
    lo = Vector((min(v.x for v in vs), min(v.y for v in vs), min(v.z for v in vs)))
    hi = Vector((max(v.x for v in vs), max(v.y for v in vs), max(v.z for v in vs)))
    return lo, hi


def tri_count(ob):
    return sum(len(p.vertices) - 2 for p in ob.data.polygons)


# =====================================================================
#  BUILD: BODY
# =====================================================================

def build_body():
    body = prism("body", PROFILE, 'X', -W / 2.0, W / 2.0)
    fillet(body, FILLET, FILLET_SEG, FILLET_ANGLE)

    cutters = []

    # --- battery bay + door opening (one through pocket) --------------
    cutters.append(box("bat_bay", -BAY_HW, BAY_HW, BAY_Y0, BAY_Y1, -2.0, BAY_Z1))

    # detent grooves for the door
    for sgn in (-1, 1):
        for cy in DETENT_Y:
            x0 = sgn * BAY_HW
            x1 = sgn * (BAY_HW + DETENT_D)
            cutters.append(box(f"detent{sgn}{int(cy)}", min(x0, x1), max(x0, x1),
                               cy - DETENT_LEN / 2, cy + DETENT_LEN / 2,
                               DOOR_T / 2 - 0.3, DOOR_T / 2 + 0.9))
    # finger notch to pop the door out
    cutters.append(cyl("door_notch", 6.5, 0.0, BAY_Y1 + 1.5, -2.0, DOOR_T + 0.1))

    # --- internal void (weight + wiring + RC522 insertion path) -------
    lb = ((0.0, 0.0), (1.0, 0.0))                                   # Z = 0
    lf = seg_line((0.0, BOT_CHAMFER), (0.0, NOSE_H), FRONT_INNER)   # front plinth
    ln = seg_line((0.0, NOSE_H), BROW, FRONT_INNER)                 # NFC face
    ld = seg_line(BROW, TOPR, DISP_INNER)                           # display panel
    lr = seg_line(TOPR, (D, REAR_PLINTH_H), WALL)                   # rear rake
    lp = seg_line((D, REAR_PLINTH_H), (D, BOT_CHAMFER), WALL)       # rear plinth
    lines = [lb, lf, ln, ld, lr, lp]
    inner = [line_isect(lines[i], lines[(i + 1) % len(lines)])
             for i in range(len(lines))]
    inner = inner[-1:] + inner[:-1]          # start at lb ^ lf

    void = prism("void", inner, 'X', -VOID_HW, VOID_HW)
    gable = prism("gable", [(-VOID_HW, -50.0), (VOID_HW, -50.0),
                            (VOID_HW, VOID_Z0 + VOID_SIDE_H), (0.0, VOID_APEX),
                            (-VOID_HW, VOID_Z0 + VOID_SIDE_H)],
                  'Y', -50.0, D + 50.0)
    boolean(void, gable, 'INTERSECT')
    floor = box("void_floor", -60, 60, -50, D + 50, VOID_Z0, H + 50)
    boolean(void, floor, 'INTERSECT')
    cutters.append(void)

    # --- display slot on the tilted panel -----------------------------
    org, ang = BROW, -_dis_a   # negative angle: panel slopes DOWN toward the rear (vendor)
    cutters.append(panel_rect("pkt_main", PKT_U0, PKT_U1 - PINCH_LEN, PKT_W / 2,
                              PKT_W0, PKT_W1, org, ang))
    cutters.append(panel_rect("pkt_pinch", PKT_U1 - PINCH_LEN, PKT_U1,
                              (PKT_W - PINCH) / 2, PKT_W0, PKT_W1, org, ang))
    # wiring / connector relief under the board's trailing end
    cutters.append(panel_rect("pkt_relief", PKT_U1 - PINCH_LEN - 2.0, PKT_U1,
                              20.0, PKT_W1, PKT_W1 - 3.1, org, ang))
    # screen window through the bezel lip
    cutters.append(panel_rect("window", WIN_U0, WIN_U1, WIN_W / 2,
                              2.0, PKT_W0 - 0.4, org, ang))
    # full-face glass recess (the dark "screen panel" look)
    cutters.append(panel_rect("glass", PKT_U0 - 1.0, PKT_U0 - 1.0 + GLASS_L,
                              GLASS_W / 2, 0.5, -GLASS_DEPTH, org, ang))
    # USB-C cable relief in the rear rake, under the slot mouth
    cutters.append(box("usb", -USB_W / 2, USB_W / 2,
                       TOPR[0] - 4.0, D + 2.0, USB_Z, USB_Z + USB_H))

    # --- vertical wire spine: void -> board connector relief ----------
    spine_top = (BROW[1] + (SPINE_Y - BROW[0]) * math.tan(_dis_a)
                 - (BEZEL_T + PKT_T + 3.1) * math.cos(_dis_a) + 2.0)
    cutters.append(cyl("spine", SPINE_R, 0.0, SPINE_Y,
                       VOID_APEX - 13.0, spine_top))

    # --- RC522 pocket on the front face -------------------------------
    org, ang = (0.0, NOSE_H), _nfc_a
    cutters.append(panel_rect("nfc_pkt", NFCP_U0, NFCP_U1, NFC_PW / 2,
                              -NFC_SKIN, -(NFC_SKIN + NFC_PT), org, ang))
    cutters.append(panel_prism("nfc_duct",
                               circle_poly(NFC_DUCT_U, 0.0, NFC_DUCT_R),
                               -NFC_SKIN, -(NFC_SKIN + NFC_BACK_WALL + 20.0),
                               org, ang))

    # --- contactless mark ---------------------------------------------
    cutters.append(panel_prism("mark_circle", circle_poly(MARK_U, 0.0, MARK_R),
                               0.5, -MARK_DEPTH, org, ang))
    for i, r in enumerate(ARC_RADII):
        cutters.append(panel_prism(
            f"arc{i}", arc_poly(MARK_U - 5.0, 0.0, r - ARC_T / 2, r + ARC_T / 2,
                                90 - ARC_SPAN, 90 + ARC_SPAN),
            0.5, -ARC_DEPTH, org, ang))

    # --- rubber feet ---------------------------------------------------
    for sx in (-FOOT_X, FOOT_X):
        for fy in FOOT_Y:
            cutters.append(cyl(f"foot{int(sx)}{int(fy)}", FOOT_R, sx, fy,
                               -2.0, FOOT_DEPTH))

    for c in cutters:
        boolean(body, c, 'DIFFERENCE')

    # --- positive retention features (UNION back into the cavities) ---
    org_disp, ang_disp = BROW, -_dis_a
    org_nfc, ang_nfc = (0.0, NOSE_H), _nfc_a
    feats = []

    # battery anti-rattle ribs (4 vertical ribs pressing the battery)
    for sx in (-1, 1):
        for cy in (BAY_Y0 + 25, BAY_Y1 - 25):
            x0 = sx * BAY_HW
            x1 = sx * (BAY_HW - 0.5)
            feats.append(box(f"rib{sx}{int(cy)}", min(x0, x1), max(x0, x1),
                             cy - 12, cy + 12, DOOR_T, BAY_Z1))

    # board snap bump (clicks over the board trailing edge, deep side)
    feats.append(panel_rect("board_snap", PKT_U0 + PKT_L - 1.0, PKT_U0 + PKT_L,
                            12.0, PKT_W1, PKT_W1 + 0.6, org_disp, ang_disp))

    # NFC clip (lip at the pocket back edge, keeps the RC522 seated)
    feats.append(panel_rect("nfc_clip", NFCP_U1 - 0.5, NFCP_U1 + 0.5,
                            NFC_PW / 2 - 3.0, -(NFC_SKIN + NFC_PT),
                            -(NFC_SKIN + NFC_PT) + 0.8, org_nfc, ang_nfc))

    for f in feats:
        boolean(body, f, 'UNION')

    return body


# =====================================================================
#  BUILD: BATTERY DOOR   (modelled flat, centred on the origin)
# =====================================================================

def build_door():
    hw = BAY_HW - DOOR_CLEAR
    hl = BAT_CAV_L / 2.0 - DOOR_CLEAR
    door = box("battery_door", -hw, hw, -hl, hl, 0.0, DOOR_T)

    bumps = []
    for sgn in (-1, 1):
        for cy in DETENT_Y:
            ly = cy - D / 2.0
            x0, x1 = sgn * hw, sgn * (hw + DETENT_PROUD)
            bumps.append(box(f"bump{sgn}{int(cy)}", min(x0, x1), max(x0, x1),
                             ly - (DETENT_LEN - 1.0) / 2,
                             ly + (DETENT_LEN - 1.0) / 2,
                             DOOR_T / 2 - 0.15, DOOR_T / 2 + 0.75))
    for b in bumps:
        boolean(door, b, 'UNION')

    fillet(door, 0.5, 2, 35.0)
    # fingernail scallop on the underside of one short edge
    boolean(door, cyl("scallop", 6.5, 0.0, hl + 2.0, -1.0, 1.2), 'DIFFERENCE')
    return door


# =====================================================================
#  EXPORT / REPORT
# =====================================================================

def export(ob, path):
    bpy.ops.object.select_all(action='DESELECT')
    ob.select_set(True)
    bpy.context.view_layer.objects.active = ob
    try:
        bpy.ops.wm.stl_export(filepath=path, export_selected_objects=True,
                              global_scale=1.0, ascii_format=False,
                              apply_modifiers=True)
    except AttributeError:
        bpy.ops.export_mesh.stl(filepath=path, use_selection=True,
                                global_scale=1.0)


def report(ob, path):
    lo, hi = bbox(ob)
    print(f"[{ob.name}] {os.path.basename(path)}")
    print(f"    bbox  X {lo.x:8.2f} .. {hi.x:8.2f}   ({hi.x - lo.x:6.2f} mm)")
    print(f"    bbox  Y {lo.y:8.2f} .. {hi.y:8.2f}   ({hi.y - lo.y:6.2f} mm)")
    print(f"    bbox  Z {lo.z:8.2f} .. {hi.z:8.2f}   ({hi.z - lo.z:6.2f} mm)")
    print(f"    tris  {tri_count(ob)}")


def preview(path):
    sc = bpy.context.scene
    sc.render.engine = 'BLENDER_WORKBENCH'
    sc.render.resolution_x = 1100
    sc.render.resolution_y = 800
    cam_d = bpy.data.cameras.new("cam")
    cam_d.type = 'ORTHO'
    cam_d.ortho_scale = 230
    cam = bpy.data.objects.new("cam", cam_d)
    bpy.context.collection.objects.link(cam)
    sc.camera = cam
    sc.display.shading.light = 'STUDIO'
    sc.display.shading.show_cavity = True
    tgt = Vector((0.0, D / 2.0, H / 2.0))

    # Vue avant (côté client / NFC)
    cam.location = tgt + Vector((-165.0, -150.0, 105.0))
    cam.rotation_euler = (math.radians(66), 0, math.radians(-49))
    sc.render.filepath = os.path.join(OUT_DIR, "preview_front.png")
    bpy.ops.render.render(write_still=True)

    # Vue arrière (côté vendeur / écran) — rotation 180° autour de Z
    cam.location = tgt + Vector((165.0, 150.0, 105.0))
    cam.rotation_euler = (math.radians(66), 0, math.radians(131))
    sc.render.filepath = path
    bpy.ops.render.render(write_still=True)


def section_render(path):
    sc = bpy.context.scene
    sc.render.engine = 'BLENDER_WORKBENCH'
    sc.render.resolution_x = 1100
    sc.render.resolution_y = 800
    cam_d = bpy.data.cameras.new("cam")
    cam_d.type = 'ORTHO'
    cam_d.ortho_scale = 160
    cam = bpy.data.objects.new("cam", cam_d)
    bpy.context.collection.objects.link(cam)
    sc.camera = cam
    sc.display.shading.light = 'STUDIO'
    sc.display.shading.show_cavity = True
    cam.location = Vector((80.0, D / 2.0, H / 2.0))
    cam.rotation_euler = (math.radians(90), 0, math.radians(90))
    sc.render.filepath = path
    bpy.ops.render.render(write_still=True)


def main():
    bpy.ops.wm.read_factory_settings(use_empty=True)

    body = build_body()
    door = build_door()

    body_path = os.path.join(OUT_DIR, "body.stl")
    door_path = os.path.join(OUT_DIR, "battery_door.stl")

    print("\n================ LightningPoS enclosure ================")
    print(f"NFC face slope   {NFC_SLOPE:6.2f} mm  (module needs {NFC_PL:.1f})")
    print(f"Display slope    {DISP_SLOPE:6.2f} mm  (needs {PKT_L + PCB_END_WALL:.1f})")
    print(f"Brow             Y={BROW[0]:.2f}  Z={BROW[1]:.2f}")
    print(f"Top rear edge    Y={TOPR[0]:.2f}  Z={TOPR[1]:.2f}")
    rake = math.degrees(math.atan2(H - REAR_PLINTH_H, D - TOPR[0]))
    print(f"Rear rake        {rake:.1f} deg from horizontal (>=45 = no supports)")
    print(f"NFC face         {90 - NFC_TILT:.1f} deg from horizontal")
    print(f"Void apex        Z={VOID_APEX:.2f}")
    print("--------------------------------------------------")

    export(body, body_path)
    export(door, door_path)
    report(body, body_path)
    report(door, door_path)
    print("==================================================\n")

    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if "--section" in argv:
        cutter = box("sec", 0.0, 80.0, -50.0, D + 50.0, -50.0, H + 50.0)
        boolean(body, cutter, 'DIFFERENCE')
        section_render(os.path.join(OUT_DIR, "section.png"))
    if "--preview" in argv:
        preview(os.path.join(OUT_DIR, "preview.png"))


if __name__ == "__main__":
    main()