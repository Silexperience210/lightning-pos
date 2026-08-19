#!/usr/bin/env python3
"""Vérifie que les ponts d'ancrage arrière n'intersectent pas la porte."""
import os
import sys

import numpy as np
import trimesh

here = os.path.dirname(os.path.abspath(__file__))
body_path = os.path.join(here, 'body.stl')
door_path = os.path.join(here, 'battery_door.stl')

body = trimesh.load_mesh(body_path)
door = trimesh.load_mesh(door_path)

# Région des deux ponts d'ancrage : x 24..29.5 autour de ±26.75,
# y 37..46.5, z 0..1.5.
grid = 0.5
xs = np.arange(24.0, 29.5 + grid, grid)
ys = np.arange(37.0, 46.5 + grid, grid)
zs = np.arange(0.1, 1.5, grid)
points = np.array([[x, y, z] for x in xs for y in ys for z in zs])

# Points à l'intérieur du corps (pont) et à l'intérieur de la porte
in_body = body.contains(points)
in_door = door.contains(points)
conflict = np.logical_and(in_body, in_door).sum()
print(f"Points échantillonnés dans le volume pont : {len(points)}")
print(f"Dans le corps (pont) : {in_body.sum()}")
print(f"Dans la porte        : {in_door.sum()}")
print(f"Conflits porte/pont  : {conflict}")

if conflict:
    bad = points[np.logical_and(in_body, in_door)]
    print("Points en conflit :")
    for p in bad:
        print(f"  x={p[0]:.2f} y={p[1]:.2f} z={p[2]:.2f}")
    sys.exit(1)

print("OK : aucune intersection entre les ponts et la porte.")
sys.exit(0)
