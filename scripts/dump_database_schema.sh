#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Writes production's public schema, without its data or owners, for the database tests in tests/databaseForTests.ts.
# Run it after a migration reaches production. It only reads, and needs pg_dump 17 like production (brew install postgresql@17).
pg_dump --version | grep -q ' 17\.' || { echo "pg_dump 17 must come first on PATH, for example PATH=/opt/homebrew/opt/postgresql@17/bin:\$PATH" >&2; exit 1; }
url=$(. packages/website/.env.local && printf '%s' "$DRIZZLE_DATABASE_URL")

# The last migration production has is the last one in the journal whose file hash production recorded.
applied_hashes=$(psql "$url" -X -A -t -c 'select hash from drizzle.__drizzle_migrations')
last_migration=''
for tag in $(node -e 'for (const entry of require("./packages/website/supabase/migrations/meta/_journal.json").entries) console.log(entry.tag)'); do
  hash=$(shasum -a 256 "packages/website/supabase/migrations/$tag.sql" | cut -d' ' -f1)
  if grep -qx "$hash" <<< "$applied_hashes"; then last_migration=$tag; fi
done

{
  echo "-- Production's public schema after migration $last_migration, written by scripts/dump_database_schema.sh."
  pg_dump "$url" --schema-only --no-owner --schema=public --restrict-key=kubenew
} > tests/fixtures/database_schema.sql
echo "Wrote tests/fixtures/database_schema.sql, which production has through $last_migration."
