import bpy, bmesh, os

path = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'battery_door.stl')
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
        for e in cur.link_edges:
            o = e.other_vert(cur)
            if o.index not in visited:
                stack.append(o)
    comps.append(comp)

print(f"battery_door.stl: {len(comps)} component(s)")
for i, comp in enumerate(comps):
    xs = [v.co.x for v in comp]
    ys = [v.co.y for v in comp]
    zs = [v.co.z for v in comp]
    print(f"  comp {i}: verts={len(comp)}  X={min(xs):.2f}..{max(xs):.2f}  "
          f"Y={min(ys):.2f}..{max(ys):.2f}  Z={min(zs):.2f}..{max(zs):.2f}")
bm.free()
