## Uploads and Data
### Assets
The 'Assets' page contains a bunch of public objects anyone can use in their game. If you make an object, mesh, script, skybox, etc that other people might find useful to use in their game, we encourage you to upload it there. Assets can be deleted and changed at any time. If you update an asset, it won't update in games that have already downloaded it.

To upload an asset, ask the chat, or manually drag and drop a file into the editor.

### Catalog
The 'Catalog' page is full of objects that people can buy and wear as hats. As a developer, you can think of it as the 'Assets' page, but with meshes only. A catalog item's creator can delete it on the website, and everyone who bought it loses it. It can also be unlisted: it leaves the Catalog page and can't be bought, but anyone with its id can still use it.

All catalog items (hats) have constraints and will be rejected if they do not follow them:
- Hats should be placed in coordinates as if the origin were the top center of the character's head. 
- Assume a 1x1x1 head.
- Dimensions: The widest side must be MIN_HAT_WIDEST_SIDE=0.5 to MAX_HAT_WIDEST_SIDE=2.0 units. The bounding box's center must be within MAX_HAT_CENTER_OFFSET=0.25 units within X Y Z of the origin. The bounding box must fit within MIN_HAT_BOTTOM=-0.6 to MAX_HAT_BOTTOM=0.1 units up/down from the origin.

To make a new catalog item, the chat must first create an asset, and then convert that asset to a catalog item.


### Data
The 'Data' page (hard to find, but you can get to it on some assets) is blob storage for sounds, textures, and meshes. Lots of places reference it, including most assets and catalog items. Data is immutable once created, but it can be deleted.

### Upload Constraints
Every file can be at most 16 MiB.
- **Mesh**: a .glb with embedded textures, at most 8 MiB. Assets can have at most 20,000 triangles and 60,000 vertices, and catalog items can have at most 10,000 triangles and 30,000 vertices.
- **Texture**: a PNG, JPEG, or WebP image, at most 2048 pixels per side.
- **Sound**: Ogg Vorbis, MP3, WAV, or FLAC. A sound under 10 seconds becomes an Sfx asset, and a longer one a Sound asset.
- **Material**: a color image, plus optional normal, roughness, and metalness images, each following the Texture rules and at most 64 MiB together.
- **Skybox**: a PNG, JPEG, or WebP panorama exactly twice as wide as it is tall, at most 4096 x 2048.
- **Object**: An object published from the world. Can't come from a file; use the chat to do this, it's a special tool.
