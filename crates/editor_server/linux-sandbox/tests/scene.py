# A real rigged asset exercises headless modeling, file persistence, animation, and rendering.
import bpy
import os
from mathutils import Vector
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
bpy.ops.mesh.primitive_cube_add(location=(0, 0, 1))
cube = bpy.context.object
bpy.ops.object.armature_add()
rig = bpy.context.object
rig.name = 'Rig'
cube.parent = rig
modifier = cube.modifiers.new('Rig', 'ARMATURE')
modifier.object = rig
cube.vertex_groups.new(name='Bone').add(list(range(len(cube.data.vertices))), 1, 'REPLACE')
bone = rig.pose.bones['Bone']
bone.rotation_mode = 'XYZ'
for frame, angle in [(1, 0), (8, 0.6)]:
    bone.rotation_euler.z = angle
    bone.keyframe_insert(data_path='rotation_euler', frame=frame)
scene = bpy.context.scene
scene.frame_start = 1
scene.frame_end = 8
scene.frame_set(1)
bpy.ops.object.select_all(action='DESELECT')
cube.select_set(True)
rig.select_set(True)
bpy.ops.export_scene.gltf(filepath='model.glb', export_format='GLB', use_selection=True)
bpy.ops.object.camera_add(location=(5, -6, 4))
camera = bpy.context.object
camera.rotation_euler = (Vector((0, 0, 1)) - camera.location).to_track_quat('-Z', 'Y').to_euler()
scene.camera = camera
bpy.ops.object.light_add(type='AREA', location=(3, -3, 5))
bpy.context.object.data.energy = 700
scene.render.engine = 'CYCLES'
scene.cycles.device = 'CPU'
scene.cycles.use_denoising = False
scene.cycles.samples = 4
scene.render.resolution_x = 128
scene.render.resolution_y = 128
scene.render.resolution_percentage = 100
scene.render.image_settings.file_format = 'PNG'
scene.render.filepath = os.path.abspath('frame1.png')
bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath('model.blend'), compress=False)
bpy.ops.render.render(write_still=True)
