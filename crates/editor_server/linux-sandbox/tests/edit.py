# Reopen the existing rig and add an animation without rebuilding its meshes or bones.
import bpy
import os

rig = bpy.data.objects['Rig']
assert rig.type == 'ARMATURE'
original = rig.animation_data.action
assert original is not None
original.name = 'Original'
track = rig.animation_data.nla_tracks.new()
track.name = 'Original'
track.strips.new('Original', 1, original)
edited = original.copy()
edited.name = 'Edited'
rig.animation_data.action = edited
bone = rig.pose.bones['Bone']
bone.rotation_euler.z = 1.2
bone.keyframe_insert(data_path='rotation_euler', frame=8)

bpy.ops.object.select_all(action='DESELECT')
for obj in bpy.data.objects:
    obj.select_set(obj.type in {'MESH', 'ARMATURE'})
bpy.ops.file.pack_all()
bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath('edited.blend'), compress=False)
bpy.ops.export_scene.gltf(filepath=os.path.abspath('edited.glb'), export_format='GLB', use_selection=True)
