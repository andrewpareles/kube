#!/usr/bin/env python3
"""Run the PropertyAnimation/PropertyKeyframe conversion over every world in Supabase.

Wraps migrate_world() from property_animation_objects_migration.py. That script's CLI is
built for .kube files on disk and bumps worldSchemaVersion on every file it opens, which is
wrong in bulk: a schema-1 world with no animators would come out claiming to be schema 72.
Here we only touch rows that actually carry an inline-curve PropertyAnimator, and only bump
the version on rows that were already at 71 (i.e. otherwise-current worlds). Older worlds get
their animators converted but keep their real version, so they stay queryable as stale.

Rows are updated in place. UPDATE does not fire game_worlds_set_version_num (BEFORE INSERT),
so version_num is untouched and no new world revisions are created.

  --dry-run   report what would change, write nothing
  --apply     perform the migration (makes a backup table first)
"""

import argparse
import json
import os
import re
import subprocess
import sys
import csv
import importlib.util

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
BACKUP_TABLE = "game_worlds_backup_prop_anim"

# reuse the repo's converter rather than reimplementing the shape change
spec = importlib.util.spec_from_file_location(
    "prop_anim_migration", os.path.join(HERE, "property_animation_objects_migration.py")
)
converter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(converter)


def database_url():
    for name in (".env.local", ".env", ".env.development"):
        path = os.path.join(REPO, "packages", "website", name)
        if not os.path.exists(path):
            continue
        for line in open(path):
            match = re.match(r"^DRIZZLE_DATABASE_URL\s*=\s*(.+)$", line.strip())
            if match:
                return match.group(1).strip().strip('"').strip("'")
    sys.exit("DRIZZLE_DATABASE_URL not found in packages/website/.env*")


def psql(db_url, sql, tuples_only=True):
    args = ["psql", db_url, "-v", "ON_ERROR_STOP=1", "-c", sql]
    if tuples_only:
        args[3:3] = ["-At", "-F", "\t"]
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode != 0:
        sys.exit(f"psql failed:\n{result.stderr}")
    return result.stdout


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--dry-run", action="store_true")
    group.add_argument("--apply", action="store_true")
    args = parser.parse_args()

    db_url = database_url()

    # pull every row carrying an old-style animator. CSV via \copy keeps the JSON intact.
    export_path = os.path.join(HERE, "_worlds_export.csv")
    copy_sql = (
        "\\copy (select game_id, version_num, gameworld_json from public.game_worlds "
        "where gameworld_json like '%\"PropertyAnimator\"%' "
        "and gameworld_json not like '%propertyAnimationId%' "
        "order by game_id, version_num) to '" + export_path + "' with (format csv)"
    )
    subprocess.run(["psql", db_url, "-v", "ON_ERROR_STOP=1", "-c", copy_sql],
                   capture_output=True, text=True, check=True)

    csv.field_size_limit(sys.maxsize)
    rows = list(csv.reader(open(export_path, newline="")))
    print(f"rows carrying an inline-curve PropertyAnimator: {len(rows)}")

    migrated, skipped, failed = [], [], []
    total_animators = total_keyframes = 0
    version_bumped = version_kept = 0

    for game_id, version_num, world_json in rows:
        try:
            world = json.loads(world_json)
        except json.JSONDecodeError as error:
            failed.append((game_id, version_num, f"unparseable JSON: {error}"))
            continue
        try:
            before_version = world.get("worldSchemaVersion")
            num_animators, num_keyframes = converter.migrate_world(world)
            if num_animators == 0:
                skipped.append((game_id, version_num))
                continue
            # only claim schema 72 for worlds that were otherwise current
            if before_version == converter.NEW_SCHEMA_VERSION - 1:
                world["worldSchemaVersion"] = converter.NEW_SCHEMA_VERSION
                version_bumped += 1
            else:
                version_kept += 1
            total_animators += num_animators
            total_keyframes += num_keyframes
            migrated.append((game_id, version_num, json.dumps(world), before_version))
        except Exception as error:  # noqa: BLE001 - one bad world must not stop the batch
            failed.append((game_id, version_num, f"{type(error).__name__}: {error}"))

    print(f"  would convert : {len(migrated)} rows, {total_animators} animators, {total_keyframes} keyframes")
    print(f"  version -> 72 : {version_bumped}   (kept old version: {version_kept})")
    print(f"  no animators  : {len(skipped)}")
    print(f"  FAILED        : {len(failed)}")
    for game_id, version_num, why in failed:
        print(f"    game {game_id} v{version_num}: {why}")

    if args.dry_run:
        print("\ndry run - nothing written")
        return 1 if failed else 0

    if failed:
        sys.exit("\nrefusing to apply while any world fails to convert - fix or exclude these first")

    # backup before any write
    print(f"\ncreating backup table {BACKUP_TABLE} ...")
    psql(db_url, f"drop table if exists public.{BACKUP_TABLE};")
    psql(db_url, f"create table public.{BACKUP_TABLE} as select * from public.game_worlds;")
    backup_count = psql(db_url, f"select count(*) from public.{BACKUP_TABLE};").strip()
    print(f"  backed up {backup_count} rows")

    # stage the converted JSON, then update in one statement
    staged_path = os.path.join(HERE, "_worlds_migrated.csv")
    with open(staged_path, "w", newline="") as staged_file:
        writer = csv.writer(staged_file)
        for game_id, version_num, new_json, _ in migrated:
            writer.writerow([game_id, version_num, new_json])

    psql(db_url, "drop table if exists public.game_worlds_migration_staging;")
    psql(db_url, "create table public.game_worlds_migration_staging "
                 "(game_id bigint, version_num bigint, gameworld_json text);")
    subprocess.run(
        ["psql", db_url, "-v", "ON_ERROR_STOP=1", "-c",
         "\\copy public.game_worlds_migration_staging (game_id, version_num, gameworld_json) from '"
         + staged_path + "' with (format csv)"],
        capture_output=True, text=True, check=True)

    updated = psql(db_url,
        "with updated as ("
        "  update public.game_worlds world"
        "     set gameworld_json = staging.gameworld_json"
        "    from public.game_worlds_migration_staging staging"
        "   where world.game_id = staging.game_id"
        "     and world.version_num = staging.version_num"
        "  returning 1"
        ") select count(*) from updated;").strip()
    print(f"  updated {updated} rows")

    psql(db_url, "drop table if exists public.game_worlds_migration_staging;")

    remaining = psql(db_url,
        "select count(*) from public.game_worlds "
        "where gameworld_json like '%\"PropertyAnimator\"%' "
        "and gameworld_json not like '%propertyAnimationId%';").strip()
    print(f"\nverification: rows still carrying an inline curve = {remaining} (expect 0)")
    return 0 if remaining == "0" else 1


if __name__ == "__main__":
    sys.exit(main())
