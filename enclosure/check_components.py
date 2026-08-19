import bpy
import os
import sys


def count_components(path):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.wm.stl_import(filepath=path)
    ob = bpy.context.active_object
    me = ob.data
    # Use BMesh to count separate connected components (islands)
    import bmesh
    bm = bmesh.new()
    bm.from_mesh(me)
    verts = list(bm.verts)
    visited = set()
    components = 0
    for v in verts:
        if v.index in visited:
            continue
        components += 1
        stack = [v]
        while stack:
            cur = stack.pop()
            if cur.index in visited:
                continue
            visited.add(cur.index)
            for edge in cur.link_edges:
                other = edge.other_vert(cur)
                if other.index not in visited:
                    stack.append(other)
    bm.free()
    print(f"{os.path.basename(path)}: vertices={len(me.vertices)} faces={len(me.polygons)} components={components}")
    return components


here = os.path.dirname(os.path.abspath(__file__))
body = os.path.join(here, 'body.stl')
door = os.path.join(here, 'battery_door.stl')
body_components = count_components(body)
door_components = count_components(door)
print(f"SINGLE_COMPONENT body={body_components == 1} door={door_components == 1}")
sys.exit(0 if (body_components == 1 and door_components == 1) else 1)
