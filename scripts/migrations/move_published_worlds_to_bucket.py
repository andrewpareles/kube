#!/usr/bin/env python3
"""Move every published world still stored in game_worlds.gameworld_json into the game-worlds bucket.

Each world is uploaded as <world_id>.kube, downloaded again and compared, and only then is its row
given that world_id. Rows that already have a world_id are skipped, so it is safe to run again.

Run it after migration 0064, and once more right before 0065, which drops gameworld_json and fails
if any row still has no world_id.

  --dry-run   count the worlds that would move, write nothing
"""

import argparse
import base64
import os
import re
import ssl
import subprocess
import sys
import urllib.request
import uuid

REPO = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
BUCKET = "game-worlds"
SSL_CONTEXT = ssl.create_default_context(cafile="/etc/ssl/cert.pem")


def read_env():
    values = {}
    for line in open(os.path.join(REPO, "packages", "website", ".env")):
        match = re.match(r"^([A-Z_]+)\s*=\s*(.+)$", line.strip())
        if match:
            values[match.group(1)] = match.group(2).strip().strip('"').strip("'")
    missing = [name for name in ("DRIZZLE_DATABASE_URL", "NEXT_PUBLIC_SUPABASE_URL", "SUPABASE_SECRET_KEY") if name not in values]
    if missing:
        sys.exit(f"Missing from packages/website/.env: {', '.join(missing)}")
    return values


def psql(database_url, sql):
    result = subprocess.run(["psql", database_url, "-X", "-v", "ON_ERROR_STOP=1", "-At", "-F", "\t", "-c", sql], capture_output=True, text=True)
    if result.returncode != 0:
        sys.exit(f"psql failed:\n{result.stderr}")
    return result.stdout


def storage_request(env, method, path, body=None):
    request = urllib.request.Request(
        f"{env['NEXT_PUBLIC_SUPABASE_URL']}/storage/v1/object/{path}",
        data=body,
        method=method,
        headers={"apikey": env["SUPABASE_SECRET_KEY"], "Content-Type": "text/plain"},
    )
    with urllib.request.urlopen(request, context=SSL_CONTEXT, timeout=120) as response:
        return response.read()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    env = read_env()
    database_url = env["DRIZZLE_DATABASE_URL"]
    rows = [line.split("\t") for line in psql(database_url, "select game_id, version_num from game_worlds where world_id is null order by game_id, version_num").splitlines() if line]
    print(f"{len(rows)} worlds to move")
    if args.dry_run:
        return
    for index, (game_id, version_num) in enumerate(rows, 1):
        encoded = psql(database_url, f"select encode(convert_to(gameworld_json, 'UTF8'), 'base64') from game_worlds where game_id={int(game_id)} and version_num={int(version_num)} and world_id is null")
        if not encoded.strip():
            continue
        world = base64.b64decode("".join(encoded.split()))
        world_id = str(uuid.uuid4())
        storage_request(env, "POST", f"{BUCKET}/{world_id}.kube", world)
        if storage_request(env, "GET", f"authenticated/{BUCKET}/{world_id}.kube") != world:
            sys.exit(f"Game {game_id} version {version_num}: the uploaded world does not match, stopping")
        psql(database_url, f"update game_worlds set world_id='{world_id}' where game_id={int(game_id)} and version_num={int(version_num)} and world_id is null")
        print(f"{index}/{len(rows)} game {game_id} version {version_num} -> {world_id} ({len(world)} bytes)")
    remaining = psql(database_url, "select count(*) from game_worlds where world_id is null").strip()
    print(f"Done. {remaining} worlds still have no world_id.")


if __name__ == "__main__":
    main()
