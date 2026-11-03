# Builds rigged_character.glb, the way the AI chat makes a character in its sandbox: one mesh object skinned to one armature,
# with an Idle, a Walk and a Jump action. The bones have the default character's joint names.
# Run: blender --background --factory-startup --python-exit-code 1 --python rigged_character.py
import math
import os

import bpy

bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.render.fps = 30


def make_material(name, color):
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    principled = material.node_tree.nodes["Principled BSDF"]
    principled.inputs["Base Color"].default_value = (*color, 1.0)
    principled.inputs["Roughness"].default_value = 0.6
    return material


materials = {
    "Skin": make_material("Skin", (0.9, 0.62, 0.45)),
    "Shirt": make_material("Shirt", (1.0, 1.0, 1.0)),
    "Pants": make_material("Pants", (0.1, 0.2, 0.7)),
    "Shoes": make_material("Shoes", (0.25, 0.12, 0.05)),
}

# Each part is (bone, material, mesh maker). The parts are joined into one object at the end.
parts = []


def add_part(bone, material, make):
    make()
    part = bpy.context.active_object
    part.data.materials.append(materials[material])
    group = part.vertex_groups.new(name=bone)
    group.add(range(len(part.data.vertices)), 1.0, "REPLACE")
    parts.append(part)


add_part("Torso", "Shirt", lambda: bpy.ops.mesh.primitive_cube_add(size=1, location=(0, 0, 2.2), scale=(1.1, 0.6, 1.2)))
add_part("Hips", "Pants", lambda: bpy.ops.mesh.primitive_cube_add(size=1, location=(0, 0, 1.45), scale=(1.1, 0.6, 0.4)))
add_part("Head", "Skin", lambda: bpy.ops.mesh.primitive_uv_sphere_add(segments=16, ring_count=8, radius=0.5, location=(0, 0, 3.3)))
add_part("Head", "Skin", lambda: bpy.ops.mesh.primitive_uv_sphere_add(segments=8, ring_count=4, radius=0.12, location=(0, -0.5, 3.25)))
for side, x in (("Left", 0.75), ("Right", -0.75)):
    add_part(f"{side}Arm", "Shirt", lambda x=x: bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=0.18, depth=1.1, location=(x, 0, 2.15)))
    add_part(f"{side}Arm", "Skin", lambda x=x: bpy.ops.mesh.primitive_uv_sphere_add(segments=8, ring_count=4, radius=0.2, location=(x, 0, 1.5)))
for side, x in (("Left", 0.28), ("Right", -0.28)):
    add_part(f"{side}Leg", "Pants", lambda x=x: bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=0.22, depth=1.2, location=(x, 0, 0.75)))
    add_part(f"{side}Leg", "Shoes", lambda x=x: bpy.ops.mesh.primitive_cube_add(size=1, location=(x, -0.12, 0.1), scale=(0.4, 0.65, 0.2)))

bpy.ops.object.select_all(action="DESELECT")
for part in parts:
    part.select_set(True)
bpy.context.view_layer.objects.active = parts[0]
bpy.ops.object.join()
body = bpy.context.active_object
body.name = "Robot"
body.data.name = "Robot"
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

# The bones copy the default character's skeleton: Hips at the waist, Torso above it, Head on top, and the arms and legs
# hanging down from the shoulders and hips.
bpy.ops.object.armature_add(location=(0, 0, 0))
armature = bpy.context.active_object
armature.name = "Armature"
bpy.ops.object.mode_set(mode="EDIT")
edit_bones = armature.data.edit_bones
edit_bones.remove(edit_bones[0])


def add_bone(name, head, tail, parent):
    bone = edit_bones.new(name)
    bone.head = head
    bone.tail = tail
    bone.parent = edit_bones[parent] if parent else None
    return bone


add_bone("Hips", (0, 0, 1.6), (0, 0, 1.9), None)
add_bone("Torso", (0, 0, 1.9), (0, 0, 2.8), "Hips")
add_bone("Head", (0, 0, 2.8), (0, 0, 3.6), "Torso")
add_bone("LeftArm", (0.75, 0, 2.7), (0.75, 0, 1.6), "Torso")
add_bone("RightArm", (-0.75, 0, 2.7), (-0.75, 0, 1.6), "Torso")
add_bone("LeftLeg", (0.28, 0, 1.4), (0.28, 0, 0.1), "Hips")
add_bone("RightLeg", (-0.28, 0, 1.4), (-0.28, 0, 0.1), "Hips")
bpy.ops.object.mode_set(mode="OBJECT")

body.parent = armature
modifier = body.modifiers.new("Armature", "ARMATURE")
modifier.object = armature

# Each action keys the bones' rotations at a few frames and loops back to its first pose.
def make_action(name, frame_count, rotation_of_bone_at):
    armature.animation_data_create()
    action = bpy.data.actions.new(name)
    armature.animation_data.action = action
    for frame in range(0, frame_count + 1, max(1, frame_count // 8)):
        t = frame / frame_count
        for pose_bone in armature.pose.bones:
            pose_bone.rotation_mode = "XYZ"
            pose_bone.rotation_euler = rotation_of_bone_at(pose_bone.name, t)
            pose_bone.keyframe_insert("rotation_euler", frame=frame)
    track = armature.animation_data.nla_tracks.new()
    track.name = name
    track.strips.new(name, 1, action)
    armature.animation_data.action = None


def swing(t, amount):
    return math.radians(amount) * math.sin(2 * math.pi * t)


def walk(bone, t):
    swing_of_bone = {"LeftArm": swing(t, 35), "RightArm": -swing(t, 35), "LeftLeg": -swing(t, 35), "RightLeg": swing(t, 35)}
    return (swing_of_bone.get(bone, 0.0), 0.0, 0.0)


def idle(bone, t):
    return (swing(t, 4) if bone == "Torso" else 0.0, 0.0, 0.0)


def jump(bone, t):
    rotation_of_bone = {"LeftArm": (math.radians(150), 0, 0), "RightArm": (math.radians(150), 0, 0), "LeftLeg": (math.radians(-25), 0, 0), "RightLeg": (math.radians(20), 0, 0)}
    return rotation_of_bone.get(bone, (0.0, 0.0, 0.0))


make_action("Idle", 60, idle)
make_action("Walk", 30, walk)
make_action("Jump", 30, jump)

bpy.ops.object.select_all(action="DESELECT")
body.select_set(True)
armature.select_set(True)
bpy.ops.export_scene.gltf(
    filepath=os.path.join(os.path.dirname(os.path.abspath(__file__)), "rigged_character.glb"),
    export_format="GLB",
    use_selection=True,
    export_animations=True,
    export_animation_mode="ACTIONS",
)
