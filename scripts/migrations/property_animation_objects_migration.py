#!/usr/bin/env python3
"""Convert .kube worlds from inline PropertyAnimator curves to PropertyAnimation/PropertyKeyframe objects.

See property_animation_objects_migration.md for the shape change and the caveats.
Idempotent: worlds already at schema 72, or with no property animators, are left alone.
"""

import argparse
import json
import sys

NEW_SCHEMA_VERSION = 72


def migrate_world(world):
    object_of_id = world["objectOfId"]
    children_of_id = world["childrenOfId"]

    # objectOfId keys are strings in the file but ids are ints in the values
    parent_of_id = {}
    for parent_id, child_ids in children_of_id.items():
        for child_id in child_ids:
            parent_of_id[child_id] = int(parent_id)

    animator_ids = [
        int(object_id)
        for object_id, live_object in object_of_id.items()
        if "PropertyAnimator" in live_object["info"] and "animation" in live_object["info"]["PropertyAnimator"]
    ]
    if not animator_ids:
        return 0, 0

    next_id = max(int(object_id) for object_id in object_of_id) + 1

    def add_object(name, info, parent_id):
        nonlocal next_id
        object_id = next_id
        next_id += 1
        object_of_id[str(object_id)] = {"id": object_id, "name": name, "info": info}
        children_of_id[str(object_id)] = []
        children_of_id.setdefault(str(parent_id), []).append(object_id)
        return object_id

    num_keyframes = 0
    for animator_id in sorted(animator_ids):
        animator_object = object_of_id[str(animator_id)]
        animator_info = animator_object["info"]["PropertyAnimator"]
        animation = animator_info.pop("animation")

        # the animation object sits beside the animator; it is found by id, not by tree position
        parent_id = parent_of_id.get(animator_id, world["rootObjectId"])
        property_animation_id = add_object(
            f"{animator_object['name']}_animation",
            {"PropertyAnimation": {
                "duration": animation["duration"],
                "interpolationStyle": animation["interpolationStyle"],
            }},
            parent_id,
        )

        # keyframe order is not preserved by the runtime (children are a HashSet); time is what matters
        for keyframe_index, keyframe in enumerate(animation.get("keyframes") or []):
            add_object(
                f"{animator_object['name']}_kf{keyframe_index}",
                {"PropertyKeyframe": {"time": keyframe["time"], "value": keyframe["value"]}},
                property_animation_id,
            )
            num_keyframes += 1

        # every migrated animator gets its own animation, never a shared one
        animator_info["propertyAnimationId"] = property_animation_id

    return len(animator_ids), num_keyframes


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("paths", nargs="+", help=".kube files to convert in place")
    parser.add_argument("--dry-run", action="store_true", help="report changes without writing")
    args = parser.parse_args()

    exit_code = 0
    for path in args.paths:
        try:
            with open(path) as world_file:
                world = json.load(world_file)
        except (OSError, json.JSONDecodeError) as error:
            print(f"{path}: could not read as JSON: {error}", file=sys.stderr)
            exit_code = 1
            continue

        schema_version = world.get("worldSchemaVersion")
        if schema_version is not None and schema_version >= NEW_SCHEMA_VERSION:
            print(f"{path}: already at schema {schema_version}, skipped")
            continue

        num_animators, num_keyframes = migrate_world(world)
        world["worldSchemaVersion"] = NEW_SCHEMA_VERSION

        if args.dry_run:
            print(f"{path}: would convert {num_animators} animator(s), {num_keyframes} keyframe(s), and set schema {NEW_SCHEMA_VERSION}")
            continue

        with open(path, "w") as world_file:
            json.dump(world, world_file, indent=2)
        print(f"{path}: converted {num_animators} animator(s), {num_keyframes} keyframe(s), schema -> {NEW_SCHEMA_VERSION}")

    return exit_code


if __name__ == "__main__":
    sys.exit(main())
