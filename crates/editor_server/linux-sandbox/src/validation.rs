//! Check an untrusted .glb before the editor's glTF importer allocates mesh/texture data.
use anyhow::{Context, Result, ensure};
use std::io::Cursor;

pub fn validate_glb(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() <= crate::MAX_GLB_BYTES && bytes.starts_with(b"glTF"),
        "Expected a bounded binary glTF model."
    );
    let gltf = gltf::Gltf::from_slice(bytes)?;
    let binary = gltf.blob.as_deref().context("GLB has no embedded data.")?;
    // No host paths or URLs may be passed through to gltf::import_slice.
    ensure!(gltf.buffers().count() == 1, "GLB must have one embedded buffer.");
    for buffer in gltf.buffers() {
        match buffer.source() {
            gltf::buffer::Source::Bin => ensure!(buffer.length() <= binary.len(), "GLB buffer exceeds its data."),
            gltf::buffer::Source::Uri(_) => anyhow::bail!("External GLB buffers are not allowed."),
        }
    }
    for view in gltf.views() {
        let end = view.offset().checked_add(view.length()).context("Buffer view size overflow")?;
        ensure!(end <= binary.len(), "Buffer view exceeds GLB data.");
    }
    // TODO: AI-PICKED-VALUE: Bound parser work and allocations before importing uploaded models.
    ensure!(gltf.nodes().count() <= 512 && gltf.meshes().count() <= 128, "Model has too many objects.");
    let mut accessor_count = 0usize;
    for accessor in gltf.accessors() {
        accessor_count = accessor_count.checked_add(accessor.count()).context("Accessor count overflow")?;
        ensure!(accessor_count <= 2_000_000, "Model has too much geometry or animation data.");
    }
    // The existing importer expands animation channels into poses for each skeleton joint.
    let mut animation_work = 0usize;
    for animation in gltf.animations() {
        let mut samples = 0usize;
        for channel in animation.channels() {
            samples = samples.checked_add(channel.sampler().input().count()).context("Animation size overflow")?;
        }
        for skin in gltf.skins() {
            let poses = samples.checked_mul(skin.joints().count()).context("Animation pose count overflow")?;
            animation_work = animation_work.checked_add(poses).context("Animation size overflow")?;
            ensure!(animation_work <= 100_000 && samples <= 10_000, "Model has too much animation data.");
        }
    }
    let mut vertex_count = 0usize;
    let mut index_count = 0usize;
    for node in gltf.nodes() {
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                let positions = primitive.get(&gltf::Semantic::Positions).context("Mesh has no vertex positions")?;
                vertex_count = vertex_count.checked_add(positions.count()).context("Vertex count overflow")?;
                index_count = index_count
                    .checked_add(primitive.indices().map_or(positions.count(), |indices| indices.count()))
                    .context("Index count overflow")?;
                ensure!(vertex_count <= 200_000 && index_count <= 1_200_000, "Model exceeds the geometry budget.");
            }
        }
    }
    // glTF nodes must form a forest, without cycles or shared children that expand during import.
    let mut parent_of_node = vec![None; gltf.nodes().count()];
    for node in gltf.nodes() {
        for child in node.children() {
            ensure!(
                parent_of_node[child.index()].replace(node.index()).is_none(),
                "A GLB node has multiple parents."
            );
        }
    }
    for node in gltf.nodes() {
        let mut parent = Some(node.index());
        for _ in 0..=parent_of_node.len() {
            let Some(index) = parent else {
                break;
            };
            parent = parent_of_node[index];
        }
        ensure!(parent.is_none(), "GLB contains a node cycle.");
    }
    let mut total_pixels = 0u64;
    for image in gltf.images() {
        let gltf::image::Source::View { view, mime_type } = image.source() else {
            anyhow::bail!("GLB textures must be embedded buffer views.");
        };
        ensure!(mime_type == "image/png" || mime_type == "image/jpeg", "Unsupported GLB texture format.");
        let end = view.offset().checked_add(view.length()).context("Texture size overflow")?;
        let data = binary.get(view.offset()..end).context("Texture exceeds GLB data")?;
        let (width, height) = image::ImageReader::new(Cursor::new(data)).with_guessed_format()?.into_dimensions()?;
        ensure!(
            width > 0 && height > 0 && width <= 2048 && height <= 2048,
            "Texture exceeds 2048 pixels per side."
        );
        total_pixels += u64::from(width) * u64::from(height);
        ensure!(total_pixels <= 16 * 1024 * 1024, "Model has too many texture pixels.");
    }
    Ok(())
}
