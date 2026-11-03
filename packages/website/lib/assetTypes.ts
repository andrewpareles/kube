import type { assetMetadata } from '@/supabase/migrations/schema'

export type AssetType = typeof assetMetadata.$inferSelect.assetType
export const assetTypes: AssetType[] = ['Mesh', 'Texture', 'Sound', 'Sfx', 'Object', 'Material', 'Skybox']
