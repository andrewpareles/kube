import { notFound } from 'next/navigation'
import { AssetScenePreview } from '@/components/site/AssetScenePreview'
import { readDataId, readMeshInfo } from '@/lib/siteData'

// Keep this in sync with DEHYDRATED_GLTF_CURRENT_VERSION in crates/game_types/src/consts.rs.
const DEHYDRATED_GLTF_CURRENT_VERSION = 6

export const generateMetadata = async ({ params }: { params: Promise<{ data_id: string }> }) => ({ title: `Mesh Data - ${(await params).data_id}` })

// A mesh file has no material or skeleton of its own, so it is shown grey on an empty skeleton. The skeleton has none of
// an animated mesh's joints, which leaves the mesh in the pose it was modeled in.
export default async function Page({ params }: { params: Promise<{ data_id: string }> }) {
    const dataId = readDataId((await params).data_id)
    const mesh = dataId === null ? null : await readMeshInfo(dataId)
    if (dataId === null || mesh === null) notFound()
    const model = { Mesh: { version: DEHYDRATED_GLTF_CURRENT_VERSION, blender_id: null, import_scale: 1, dehydrated_meshes: [{ name: 'mesh_1', mesh_data_id: dataId, skinned_skeleton_index: 0, material: null }], dehydrated_skeletons: [{ skeleton_index: 0, jointOfIndex: [], animations: [] }] } }

    return (
        <main className="site-container site-page">
            <h1 className="mb-2 text-4xl font-semibold tracking-tight">Mesh Data - {dataId}</h1>
            <p className="mb-1 text-sm text-gray-500">{mesh.stats === null ? `Mesh format version ${mesh.version}, which the engine no longer reads` : `${mesh.stats.size.map(length => length.toFixed(2)).join(' × ')} · ${mesh.stats.vertexCount.toLocaleString()} vertices · ${mesh.stats.triangleCount.toLocaleString()} triangles · ${mesh.stats.jointCount === null ? 'Static' : `Animated, ${mesh.stats.jointCount.toLocaleString()} joints`}`}</p>
            <p className="mb-6 text-sm text-gray-500">This shows up in the editor as Data Id {dataId}.</p>
            <AssetScenePreview assetName={`Mesh Data - ${dataId}`} assetDataJson={JSON.stringify(model)} mode="detail" controls={null} footer={null} isInspectable={true} />
        </main>
    )
}
