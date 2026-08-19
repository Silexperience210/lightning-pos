#!/usr/bin/env python3
"""Génère les sprites 3D de l'éclair LightningPoS — numpy pur (pas de PIL).

Extrude la silhouette de bolt.rs, rotation Y + bascule Z, projection
perspective douce, painter's algorithm, ombrage plat ambre, halo par
box-blur numpy. Sortie : bolt_sprites.bin (RGB565 128x128 x36) + sheet.
"""
import math, struct
import numpy as np

W = H = 128
N = 36
THICK = 16.0
SHAPE = [(7.0, -50.0), (-33.0, 2.0), (-1.0, 2.0), (-9.0, 50.0), (33.0, -6.0), (1.0, -6.0)]
NPT = len(SHAPE)

def rot_y(p, a):
    ca, sa = math.cos(a), math.sin(a)
    x, y, z = p
    return (x * ca + z * sa, y, -x * sa + z * ca)

def rot_z(p, a):
    ca, sa = math.cos(a), math.sin(a)
    x, y, z = p
    return (x * ca - y * sa, x * sa + y * ca, z)

def build():
    pts2d = [(x * 0.68, y * 0.68) for (x, y) in SHAPE]
    verts = []
    for z in (-THICK / 2, THICK / 2):
        for (x, y) in pts2d:
            verts.append((x, y, z))
    return verts

def make_faces():
    faces = []
    faces.append((list(range(NPT)), -1))
    faces.append((list(range(NPT, 2 * NPT)), 1))
    for i in range(NPT):
        j = (i + 1) % NPT
        faces.append(([i, j, NPT + j, NPT + i], 0))
    return faces

def box_blur(img, r):
    """Séparable box blur (3 passes) sur un canal float."""
    out = img.copy()
    k = np.ones(r) / r
    for _ in range(2):
        out = np.apply_along_axis(lambda m: np.convolve(m, k, mode='same'), 1, out)
        out = np.apply_along_axis(lambda m: np.convolve(m, k, mode='same'), 0, out)
    return out

def pip_mask(poly, w=W, h=H):
    """Masque point-dans-polygone (even-odd) sur la grille."""
    xs = np.arange(w)[None, :]
    ys = np.arange(h)[:, None]
    mask = np.zeros((h, w), dtype=bool)
    n = len(poly)
    for i in range(n):
        x1, y1 = poly[i]
        x2, y2 = poly[(i + 1) % n]
        if y1 == y2:
            continue
        cond = ((ys >= min(y1, y2)) & (ys < max(y1, y2)) &
                (xs < x1 + (ys - y1) * (x2 - x1) / (y2 - y1)))
        mask ^= cond
    return mask

def frame(ang):
    verts = build()
    vrot = [rot_z(rot_y(v, ang), math.sin(ang * 2) * 0.06) for v in verts]
    f = 260.0
    cx, cy = W / 2, H / 2 + 3
    def proj(p):
        x, y, z = p
        s = f / (f - z)
        return (cx + x * s * 0.9, cy + y * s * 0.9)
    faces = make_faces()
    scored = []
    for fi, (idx, kind) in enumerate(faces):
        pz = sum(vrot[i][2] for i in idx) / len(idx)
        scored.append((pz, idx, kind))
    scored.sort(reverse=True)
    img = np.zeros((H, W, 3), dtype=np.float32)
    light = np.array([0.35, 0.5, 0.79])
    light = light / np.linalg.norm(light)
    for pz, idx, kind in scored:
        poly = [proj(vrot[i]) for i in idx]
        m = pip_mask(poly)
        if kind == 1:
            col = np.array([1.0, 0.965, 0.82])
        elif kind == -1:
            col = np.array([0.22, 0.13, 0.03])
        else:
            a = np.array(vrot[idx[0]]); b = np.array(vrot[idx[1]]); c3 = np.array(vrot[idx[2]])
            u = b - a; v = c3 - a
            nvec = np.cross(u, v)
            nl = np.linalg.norm(nvec) or 1
            d = float(np.dot(nvec / nl, light))
            sh = 0.22 + 0.78 * max(0.0, d)
            col = np.array([1.0, 0.65, 0.24]) * sh
            col[2] *= 0.8
        img[m] = col
    # halo ambre (blur du masque silhouette)
    smask = np.zeros((H, W), dtype=np.float32)
    smask[pip_mask([proj(vrot[i]) for i in range(NPT)])] = 1.0
    glow = box_blur(smask, 3) * 0.12 + box_blur(smask, 6) * 0.22
    glow = np.clip(glow, 0, 1)[:, :, None]
    img = img * (1 - glow) + np.array([1.0, 0.55, 0.15]) * glow
    # dégradé vertical sur le bol (blanc haut → or bas)
    grad = np.linspace(1.0, 0.45, H)[:, None, None]
    bol = np.array([1.0, 0.94, 0.78]) * grad
    img = np.where(smask[:, :, None] > 0.5, bol, img)
    img = np.clip(img * 255, 0, 255).astype(np.uint8)
    return img, smask

def to_rgb565(img):
    r = img[:, :, 0].astype(np.uint16)
    g = img[:, :, 1].astype(np.uint16)
    b = img[:, :, 2].astype(np.uint16)
    return ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)

def main():
    blob = bytearray()
    for i in range(N):
        img, _ = frame(2 * math.pi * i / N)
        blob += to_rgb565(img).astype('<u2').tobytes()
    with open("/home/silex/lightning-pos/firmware-espidf/assets/bolt_sprites.bin", "wb") as f:
        f.write(bytes(blob))
    print(f"OK: {N} frames {W}x{H} RGB565 = {len(blob)/1024:.0f} Ko")
    # contact sheet PNG (via numpy -> PPM -> pas de PIL)
    sheet = np.zeros((H * 6, W * 6, 3), dtype=np.uint8)
    for i in range(N):
        img, _ = frame(2 * math.pi * i / N)
        r, c = divmod(i, 6)
        sheet[r * H:(r + 1) * H, c * W:(c + 1) * W] = img
    with open("/tmp/bolt_sheet.ppm", "wb") as f:
        f.write(b"P6\n%d %d\n255\n" % (W * 6, H * 6))
        f.write(sheet.tobytes())
    print("sheet: /tmp/bolt_sheet.ppm")

if __name__ == "__main__":
    main()
