# Migrating `.kube` worlds to PropertyAnimation / PropertyKeyframe objects

Applies to worlds saved at `worldSchemaVersion` 71 or lower. Worlds saved at 72+ are already in the new shape.

## What changed

`PropertyAnimator` used to carry its whole curve inline:

```jsonc
{ "PropertyAnimator": {
    "targetObjectId": 12,
    "propertyPath": ["opacity"],
    "animation": {                       // <-- inline struct, gone in 72
      "duration": 1.0,
      "interpolationStyle": "EaseInOut",
      "keyframes": [ { "time": 0.0, "value": 0.0 },
                     { "time": 1.0, "value": 1.0 } ]
    },
    "clockId": 13, "priority": 1, "weight": 100.0, "weightTransition": null } }
```

In 72 the curve is real world objects, mirroring `SkAnimator -> SkAnimation -> SkKeyframe`:

```
PropertyAnimator --propertyAnimationId--> PropertyAnimation { duration, interpolationStyle }
                                                └── PropertyKeyframe { time, value }   (one child per keyframe)
```

```jsonc
{ "PropertyAnimator": {
    "targetObjectId": 12,
    "propertyPath": ["opacity"],
    "propertyAnimationId": 500,          // <-- replaces "animation"
    "clockId": 13, "priority": 1, "weight": 100.0, "weightTransition": null } }
```

There is deliberately no runtime compatibility shim. A version-71 world containing a `propertyAnimator` fails to
load with a serde error on the missing `propertyAnimationId` field — `worldSchemaVersion` is a bump-and-break
marker, not a migration hook. Convert the file offline instead.

## Conversion steps

For each object in `objectOfId` whose `info` is `PropertyAnimator`:

1. **Mint a `PropertyAnimation` object.** Take a fresh id (one past the current max id in `objectOfId`).
   Carry over `duration` and `interpolationStyle` verbatim from the old inline `animation`.
   Name it something traceable, e.g. `<animatorName>_animation`.
2. **Parent it.** Add it to `objectOfId`, give it a `childrenOfId` entry, and register it as a child of the
   animator's own parent (any parent works — the animator finds it by id, not by tree position — but keeping it
   next to the animator matches what the editor produces).
3. **Mint one `PropertyKeyframe` per entry** in the old `animation.keyframes`, copying `time` and `value`.
   Parent each one to the new `PropertyAnimation`.
4. **Rewrite the animator**: delete the `animation` key, add `"propertyAnimationId": <new animation id>`.
5. Bump the file's `worldSchemaVersion` to `72`.

## Things to get right

- **Keyframe order is not preserved and must not be relied on.** Children live in a `HashSet`, so the runtime
  reads them back in arbitrary order and locates the surrounding pair by `time`. Copy the keyframes in any
  order; just make sure the `time` values are correct.
- **Don't share one `PropertyAnimation` between animators during migration**, even if two animators had
  byte-identical inline curves. Sharing is supported and useful, but it is a behavior change: editing a shared
  curve later moves every animator using it, and `animationService.animatePropertyTo` rewrites the curve in
  place. Give every migrated animator its own animation object.
- **An animator with zero keyframes** still needs a `PropertyAnimation` object with no children. A null
  `propertyAnimationId` is legal but makes the animator inert.
- `childrenOfId` needs an entry for every new object, including empty `[]` for leaf keyframes.

## Running the converter

`property_animation_objects_migration.py` does all of the above. It rewrites in place, so commit or back up first.

```bash
python3 scripts/migrations/property_animation_objects_migration.py path/to/world.kube
```

Pass `--dry-run` to print what it would change without writing. Pass several paths to convert a batch.
The script is idempotent: a world already at 72, or one with no property animators, is left untouched.
