import bpy
import os
import sys


def clean(ob):
    bpy.context.view_layer.objects.active = ob
    bpy.ops.object.mode_set(mode='EDIT')
    bpy.ops.mesh.select_all(action='SELECT')
    bpy.ops.mesh.remove_doubles(threshold=0.0001)
    bpy.ops.mesh.select_all(action='DESELECT')
    bpy.ops.mesh.select_non_manifold(extend=False)
    non_manifold = sum(1 for e in ob.data.edges if e.select)
    print(f"  before fix: non-manifold edges={non_manifold}")
    if non_manifold:
        # try to fill small holes / make manifold
        bpy.ops.mesh.edge_face_add()
        bpy.ops.mesh.select_all(action='SELECT')
        bpy.ops.mesh.remove_doubles(threshold=0.0001)
        bpy.ops.mesh.normals_make_consistent(inside=False)
        bpy.ops.mesh.select_all(action='DESELECT')
        bpy.ops.mesh.select_non_manifold(extend=False)
        non_manifold = sum(1 for e in ob.data.edges if e.select)
        print(f"  after fix: non-manifold edges={non_manifold}")
    bpy.ops.object.mode_set(mode='OBJECT')
    return non_manifold


def check(path):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.wm.stl_import(filepath=path)
    ob = bpy.context.active_object
    me = ob.data
    print(f"{os.path.basename(path)}: vertices={len(me.vertices)} edges={len(me.edges)} faces={len(me.polygons)}")
    non_manifold = clean(ob)
    return non_manifold


here = os.path.dirname(os.path.abspath(__file__))
body = os.path.join(here, 'body.stl')
door = os.path.join(here, 'battery_door.stl')
b_ok = check(body) == 0
d_ok = check(door) == 0
print(f"WATERTIGHT body={b_ok} door={d_ok}")
sys.exit(0 if (b_ok and d_ok) else 1)
