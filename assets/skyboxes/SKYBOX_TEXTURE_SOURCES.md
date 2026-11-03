# Skybox Texture Sources

These are the equirectangular sky panoramas we upload as TextureData and point `WorldLightingInfo.skyboxTextureDataId` at. Worlds that set no skybox texture fall back to the gradient sky, so everything here is optional decoration on top of that.

There are three sets: 25 stylized painted skies, 8 photoreal ones at four times the resolution, and 12 HD skies drawn
from noise. They do not mix well in one world, so pick a set and stay in it. Only the HD skies are Skybox assets; the other
two sets stay uploaded because worlds that used them still point at their texture data ids.

## Source: stylized set

The 25 stylized files come from [Cloudy Skyboxes](https://opengameart.org/content/cloudy-skyboxes-0) by Screaming Brain Studios, released under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). No credit is required and there are no restrictions, commercial or otherwise.

They arrive as 2048x1024 rgb24 PNG in equirectangular projection, which is already what the uploader accepts and what `skybox.wgsl` samples, so none of them needed converting. The original filenames all end in `-512x512`, which refers to the cubemap face size the pack also ships, not to these panoramas; the names below drop it.

Every panorama in this pack mirrors its lower hemisphere from its upper one. Looking straight down shows the sky again rather than ground, which is normally hidden behind a baseplate but is worth knowing before picking one for an open world.

## Source: photoreal set

The 8 photoreal files come from the [Pure Skies](https://polyhaven.com/hdris/skies) collection on
[Poly Haven](https://polyhaven.com/), also [CC0](https://polyhaven.com/license). Sky only, with no
ground in frame, unlike most of Poly Haven's sky HDRIs.

Poly Haven ships these as HDR at up to 24K plus one tonemapped JPG. Our pipeline is RGBA8, so these
came from the tonemapped JPG, which arrives at 8192x4096 and was resized to 4096x2048. That is the
largest size worth using: the renderer transcodes to uncompressed RGBA, so 4096x2048 already costs
about 45 MB of video memory each, and 8192x4096 would cost about 179 MB. 4096 is also the ceiling
of what the view can resolve at our 70 degree field of view.

To add more, download the tonemapped JPG for a `_puresky` asset, resize to exactly 4096x2048 (the
projection needs 2:1), and import it on the Skybox tab of the editor's Import panel, which
publishes it as a Skybox asset.

## Source: HD cloudy set

The 12 HD skies are drawn by `generate_cloudy_skyboxes.py` in this folder, which runs inside Blender for numpy and
PNG saving: `Blender -b --factory-startup -P generate_cloudy_skyboxes.py -- <out_dir> <preset> 4096`. Each pixel becomes a
view direction with the same mapping as `skybox.wgsl`, so the image wraps without a seam. Clouds are fractal noise on a flat
layer above the viewer, lit from the sun's side and faded into the horizon haze, and fine detail is dropped where it would
be smaller than a pixel. Below the horizon is plain haze fading to a ground colour, with no mirrored sky. None of them has
a sun in the sky; the light direction only decides which side of each cloud is brighter. The bright and faded blue skies
use the homepage's gradients and its soft, unshaded white clouds.

They were uploaded at 4096x2048 as sRGB colour TextureData, the same as the other sets.

They are published as Skybox assets 163 to 170 and 204 to 207 ("Sky - Bright Blue Clouds" and so on), so downloading one
sets a world's sky.

## Files

All 45 are uploaded as TextureData, so the images are not kept in the repo. Point
`WorldLightingInfo.skyboxTextureDataId` at the id below.
`crates/editor_server/tests/publish_default_skyboxes.rs` publishes the HD skies as Skybox assets. The
painted and photoreal skies were dropped from the assets, and the four painted moods worth keeping (storm
gold horizon, alien green, blood red and amber sunset) were redrawn in the HD set. To get a painted or
photoreal file back, download it from its source and match it by name.

I named these from thumbnails, so rename anything that reads wrong once you see it full size.

| Name | Texture data id | Look |
| --- | --- | --- |
| sky_overcast_storm | 406 | Dark storm cloud with the sun breaking through, muted pink and grey |
| sky_hazy_gold | 402 | Tan overcast lit gold from a low sun |
| sky_dusty_dusk | 399 | Warm brown dusk haze with a small low sun |
| sky_deep_blue_noon | 397 | Deep blue with a hard white sun and dark cloud along the bottom |
| sky_blue_cloudy | 392 | Blue with white cloud banks and a bright horizon |
| sky_clear_blue_horizon | 396 | Nearly clear blue, sharp bright band at the horizon |
| sky_pink_dusk | 409 | Pink and lilac cloud at dusk |
| sky_pale_overcast | 407 | Pale grey-blue overcast with a soft sun |
| sky_storm_gold_horizon | 413 | Heavy dark cloud over a vivid yellow horizon band |
| sky_murky_green | 404 | Murky olive-grey cloud, close to night |
| sky_violet_clouds | 415 | Violet cloud with a white sun |
| sky_dim_dusk | 398 | Dim grey-purple with a weak sun |
| sky_sepia_fog | 412 | Warm beige fog, very soft |
| sky_alien_green | 394 | Vivid green, clearly not Earth |
| sky_night_amber_moon | 405 | Near black night with one amber moon |
| sky_puffy_lavender | 410 | Soft lavender-blue with a large puffy white cloud |
| sky_crimson_sunset | 395 | Red and magenta horizon under a gold sun |
| sky_teal_gold | 414 | Teal-grey cloud cut by a bright gold streak |
| sky_blood_red | 411 | Deep red cloud, heavy |
| sky_white_mist | 416 | Plain grey-white mist with a diffuse sun |
| sky_amber_sunset | 393 | Strong amber sunset behind dark cloud |
| sky_ember_sunset | 400 | Deep orange-red sunset, dark overall |
| sky_mauve_storm | 403 | Dramatic pink and mauve storm cloud |
| sky_pale_wisps | 408 | Pale yellow-grey wisps, thin overcast |
| sky_grey_blue_calm | 401 | Calm dim grey-blue cloud |

Photoreal, 4096x2048. Their asset names start with "Sky - Photo" to keep the sets apart.

| Name | Texture data id | Look |
| --- | --- | --- |
| photo_autumn_field | 417 | Blue midday sky, scattered white cloud, medium contrast |
| photo_citrus_orchard | 418 | Soft overcast daylight, low contrast |
| photo_citrus_orchard_road | 419 | Brighter partly cloudy daylight |
| photo_drakensberg_mountain | 420 | Clear high altitude blue, strong sun |
| photo_kloppenheim | 421 | Warm late afternoon, high contrast |
| photo_sunflowers | 422 | Bright summer daylight, soft cloud |
| photo_table_mountain | 423 | Clear coastal blue |
| photo_qwantani_dusk | 424 | Dusk, warm low sun near the horizon |

HD cloudy, 4096x2048.

| Name | Texture data id | Look |
| --- | --- | --- |
| hd_sunny_cumulus | 650 | Deep blue with puffy white cloud |
| hd_cirrus_blue | 651 | Bright blue with thin streaky high cloud |
| hd_golden_hour | 652 | Warm gold horizon under blue, warm-lit cloud |
| hd_soft_overcast | 653 | Soft grey-white overcast, even light |
| hd_storm_front | 654 | Dark heavy grey storm cloud |
| hd_bright_blue_clouds | 655 | The homepage's first sky: vivid blue to near white, soft white clouds |
| hd_faded_blue_clouds | 656 | The homepage's game sky: faded blue with soft white clouds |
| hd_lavender_mixed_clouds | 664 | The homepage's current sky: faded blue with white clouds over lavender ones |
| hd_storm_gold_horizon | 665 | Dark charcoal sky and pale cloud over a vivid gold horizon band |
| hd_alien_green | 666 | Vivid green with streaky pale green cloud and a bright mint horizon |
| hd_blood_red | 667 | Deep red with heavy red cloud and a pale pink horizon |
| hd_amber_sunset | 668 | Dark brown sky and amber-lit cloud over a warm orange glow |

## Other license-safe sources

- [Poly Haven Pure Skies](https://polyhaven.com/hdris/skies/pure%20skies) - [CC0](https://polyhaven.com/license). Now a primary source, see above; 59 panoramas in the collection, 8 of them pulled in so far. The other 51 are there when we want more.
- [Planet Surface Skyboxes](https://opengameart.org/content/planet-surface-skyboxes) - CC0, same author and same 2048x1024 panorama format as the pack above. 32 panoramas across 16 alien and planetary themes, which covers lava, ice, ocean and space worlds that the cloudy pack does not.
