import { defineConfig } from 'drizzle-kit';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { readEnvVar } from './readEnvVar';

const drizzle_config_file_path = fileURLToPath(import.meta.url);
const envFilePath = path.join(drizzle_config_file_path, '..', '.env.local');
const drizzle_database_url = readEnvVar({ envFilePath, envKey: 'DRIZZLE_DATABASE_URL', });

console.log('drizzle.config.ts is running...');

export default defineConfig({
  schema: './supabase/migrations/schema.ts',
  out: './supabase/migrations',
  dialect: 'postgresql',
  dbCredentials: {
    url: drizzle_database_url,
  },
});
