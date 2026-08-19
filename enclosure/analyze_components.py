import bpy
import bmesh
import os


def analyze(path):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.wm.stl_import(filepath=path)
    ob = bpy.context.active_object
    me = ob.data
    bm = bmesh.new()
    bm.from_mesh(me)
    verts = list(bm.verts)
    visited = set()
    comps = []
    for v in verts:
        if v.index in visited:
            continue
        comp = []
        stack = [v]
        while stack:
            cur = stack.pop()
            if cur.index in visited:
                continue
            visited.add(cur.index)
            comp.append(cur)
            for edge in cur.link_edges:
                other = edge.other_vert(cur)
                if other.index not in visited:
                    stack.append(other)
        comps.append(comp)

    print(f"{os.path.basename(path)}: {len(comps)} component(s)")
    for i, comp in enumerate(comps):
        xs = [v.co.x for v in comp]
        ys = [v.co.y for v in comp]
        zs = [v.co.z for v in comp]
        print(f"  comp {i}: verts={len(comp)}  X={min(xs):.2f}..{max(xs):.2f}  "
              f"Y={min(ys):.2f}..{max(ys):.2f}  Z={min(zs):.2f}..{max(zs):.2f}")
    bm.free()


here = os.path.dirname(os.path.abspath(__file__))
analyze(os.path.join(here, 'body.stl'))
