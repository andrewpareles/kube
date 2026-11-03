"""Equirectangular cloudy skyboxes for the kube.new renderer.

Run inside Blender (for numpy and PNG saving):
  Blender -b --factory-startup -P generate_cloudy_skyboxes.py -- <out_dir> <preset> [width]

Each pixel is turned into a view direction with the same mapping as skybox.wgsl:
  u = atan2(x, z) / 2pi + 0.5,  v = 0.5 - asin(y) / pi
so the image wraps seamlessly around longitude. Clouds live on a flat layer above the viewer, so they shrink and
thicken in perspective towards the horizon, where they fade into the haze.
"""
import sys, os, math
import numpy as np

args = sys.argv[sys.argv.index("--") + 1:]
OUT, PRESET = args[0], args[1]
W = int(args[2]) if len(args) > 2 else 4096
H = W // 2

def hexrgb(h):
    return np.array([int(h[i:i + 2], 16) / 255 for i in (0, 2, 4)], dtype=np.float32)

PRESETS = {
    # Every sky is drawn without a sun. `light` is only the direction the clouds are lit from, as (azimuth, elevation) in
    # degrees. A sky is either two colours (zenith and horizon) or a list of `sky_stops`, from the zenith (0) to the
    # horizon (1), with `below_stops` from the horizon (0) down to straight below (1). Shaded clouds use `lit` and
    # `shadow` and the `coverage` of the sky; `soft` clouds are the homepage's, flat white wisps with no shading, given as
    # (gain, offset) for how the noise turns into cloud.
    "sunny_cumulus": dict(zenith="2f6fd8", horizon="bcd8f2", haze="c9dcec", ground="7f97ad", light=(40, 55),
                          lit="ffffff", shadow="9fb0c8", coverage=0.48, scale=0.9, stretch=(1, 1), seed=3, octaves=6),
    "soft_overcast": dict(zenith="8d9fb6", horizon="d9e1ea", haze="d4dbe3", ground="8e98a3", light=(120, 40),
                          lit="f3f5f7", shadow="9ba6b3", coverage=0.78, scale=0.7, stretch=(1, 1), seed=11, octaves=6),
    "golden_hour": dict(zenith="3a62a8", horizon="f6c48a", haze="e9b88d", ground="6e6559", light=(250, 6),
                        lit="ffe1b8", shadow="8a7768", coverage=0.42, scale=0.85, stretch=(1, 1), seed=21, octaves=6),
    "cirrus_blue": dict(zenith="2a64cf", horizon="c3dcf3", haze="cfe0ef", ground="8099b0", light=(310, 62),
                        lit="ffffff", shadow="bccbe0", coverage=0.5, scale=1.1, stretch=(2.6, 0.8), seed=7, octaves=7),
    "storm_front": dict(zenith="3d4652", horizon="9aa3ad", haze="8b939c", ground="4d545c", light=(200, 30),
                        lit="aeb6bf", shadow="3a414b", coverage=0.82, scale=0.75, stretch=(1, 1), seed=33, octaves=6),
    # TODO: AI-PICKED-VALUE: Colours matched by eye to the painted skies of the same names, which these replace.
    "storm_gold_horizon": dict(sky_stops=[(0, "2a2c29"), (0.45, "373934"), (0.61, "4d4934"), (0.71, "8c7030"), (0.79, "c8962e"),
                                          (0.87, "e6a92e"), (0.94, "f4bb3a"), (1, "fbd055")],
                               below_stops=[(0, "e0a330"), (0.1, "6a5024"), (0.35, "2e2b22"), (1, "1a1b19")], horizon="f8c444",
                               light=(250, 25), lit="f0efe8", shadow="585a55", coverage=0.62, scale=0.75, stretch=(1, 1), seed=41,
                               octaves=6),
    "alien_green": dict(sky_stops=[(0, "27453a"), (0.45, "3a6a52"), (0.61, "4f8c68"), (0.71, "66ab80"), (0.79, "86cc9c"),
                                   (0.87, "a8e8b8"), (1, "c8ffd6")],
                        below_stops=[(0, "93d8a7"), (0.1, "4d7a5e"), (0.4, "22382b"), (1, "101a14")], horizon="c0fcd0",
                        light=(300, 20), lit="d4f7df", shadow="4f7d64", coverage=0.6, scale=1.0, stretch=(2.2, 0.8), seed=53,
                        octaves=7),
    "blood_red": dict(sky_stops=[(0, "3e1714"), (0.45, "5e221d"), (0.61, "7a2e26"), (0.71, "963c31"), (0.79, "b44e3f"),
                                 (0.87, "d2705c"), (1, "f0a088")],
                      below_stops=[(0, "c96858"), (0.1, "642a24"), (0.4, "2a1210"), (1, "130807")], horizon="f2a68e",
                      light=(240, 15), lit="e0605a", shadow="5a1c1c", coverage=0.6, scale=0.8, stretch=(1, 1), seed=61, octaves=6),
    "amber_sunset": dict(sky_stops=[(0, "2a210d"), (0.3, "3b2e11"), (0.45, "5a4316"), (0.61, "8c6420"), (0.71, "b8802a"),
                                    (0.79, "dc9a34"), (0.87, "f4ae44"), (0.94, "ff9840"), (1, "ff7a36")],
                         below_stops=[(0, "e07a34"), (0.1, "6a4318"), (0.35, "2a1e0e"), (1, "0e0a05")], horizon="ff8a3c",
                         light=(70, 4), lit="e8a858", shadow="1f170b", coverage=0.55, scale=0.85, stretch=(1.6, 1), seed=71,
                         octaves=6),
    # The two homepage skies: the bright blue it started with, and the faded game-sky blue it moved to.
    "bright_blue_clouds": dict(sky_stops=[(0, "336dff"), (0.45, "8fbcff"), (0.8, "cfe5ff"), (1, "eef6ff")],
                               below_stops=[(0, "e3efff"), (1, "9fbbe6")], horizon="eef6ff", light=(0, 90),
                               soft=(2.4, 1.0), scale=1.5, stretch=(1.8, 1), seed=5, octaves=5),
    "faded_blue_clouds": dict(sky_stops=[(0, "7aa1e6"), (0.55, "93b6eb"), (0.85, "b2cdef"), (1, "bcd5f0")],
                              below_stops=[(0, "a9c4e8"), (1, "8fabd3")], horizon="bcd5f0", light=(0, 90),
                              soft=(2.2, 0.92), scale=1.4, stretch=(1.8, 1), seed=9, octaves=5),
    # The homepage's current sky: the faded blue with white clouds over a second layer of lavender ones from the logo.
    "lavender_mixed_clouds": dict(sky_stops=[(0, "7aa1e6"), (0.55, "93b6eb"), (0.85, "b2cdef"), (1, "bcd5f0")],
                                  below_stops=[(0, "a9c4e8"), (1, "8fabd3")], horizon="bcd5f0", light=(0, 90),
                                  soft=(2.2, 0.92), under_layer=dict(colour="c7adff", soft=(2.1, 0.94), shift=(37.3, 11.9)),
                                  scale=1.4, stretch=(1.8, 1), seed=9, octaves=5),
}
P = PRESETS[PRESET]
rng = np.random.default_rng(P["seed"])

# ---------------- Directions for every pixel ----------------
u = (np.arange(W, dtype=np.float32) + 0.5) / W
v = (np.arange(H, dtype=np.float32) + 0.5) / H
lon = (u - 0.5) * 2 * math.pi
lat = (0.5 - v) * math.pi
LON, LAT = np.meshgrid(lon, lat)
dx = np.cos(LAT) * np.sin(LON)
dy = np.sin(LAT)
dz = np.cos(LAT) * np.cos(LON)

# ---------------- Sky gradient ----------------
def colour_at(stops, fraction):
    # Blends between colour stops, each a (position from 0 to 1, hex colour) pair.
    positions = [position for position, _ in stops]
    colours = np.stack([hexrgb(colour) for _, colour in stops])
    return np.stack([np.interp(fraction, positions, colours[:, channel]) for channel in range(3)], axis=-1).astype(np.float32)

hor = hexrgb(P["horizon"])
up = np.clip(dy, 0, 1)
down = np.clip(-dy, 0, 1)
if "sky_stops" in P:
    # Most of what a player sees is near the horizon, so the stops are spread out towards it.
    sky = colour_at(P["sky_stops"], 1 - up ** 0.7)
    below = colour_at(P["below_stops"], down ** 0.6)
else:
    zen, haze, ground = (hexrgb(P[k]) for k in ("zenith", "haze", "ground"))
    sky = hor + (zen - hor) * (up[..., None] ** 0.45)
    below = haze + (ground - haze) * (1 - np.exp(-down[..., None] * 6))
col = np.where(dy[..., None] >= 0, sky, below)

az, el = (math.radians(a) for a in P["light"])
light_direction = np.array([math.cos(el) * math.sin(az), math.sin(el), math.cos(el) * math.cos(az)], dtype=np.float32)

# ---------------- Clouds on a layer above the viewer ----------------
PERM = rng.permutation(256)
PERM = np.concatenate([PERM, PERM])
GRAD = rng.random(256).astype(np.float32)

def value_noise(x, y):
    xi = np.floor(x).astype(np.int64)
    yi = np.floor(y).astype(np.int64)
    xf = (x - xi).astype(np.float32)
    yf = (y - yi).astype(np.float32)
    xi &= 255; yi &= 255
    def g(ix, iy):
        return GRAD[PERM[PERM[ix] + iy]]
    sx = xf * xf * (3 - 2 * xf); sy = yf * yf * (3 - 2 * yf)
    a = g(xi, yi); b = g((xi + 1) & 255, yi); c = g(xi, (yi + 1) & 255); d = g((xi + 1) & 255, (yi + 1) & 255)
    return (a + (b - a) * sx) + ((c + (d - c) * sx) - (a + (b - a) * sx)) * sy

def fbm(x, y, octaves, footprint):
    # footprint is how much cloud-layer distance one pixel covers. Octaves finer than a couple of pixels are faded out,
    # and replaced by their average, so far clouds blur instead of breaking up into grain.
    total = np.zeros_like(x, dtype=np.float32); amp = 0.5; norm = 0; freq = 1.0
    for o in range(octaves):
        weight = np.clip(2.0 - freq * footprint * 3.0, 0, 1)
        total += amp * (weight * value_noise(x, y) + (1 - weight) * 0.5)
        norm += amp
        x = x * 2.03 + 17.1; y = y * 2.03 + 9.7; amp *= 0.5; freq *= 2.03
    return total / norm

mask = dy > 0.01
py = np.maximum(dy[mask], 0.01)
# Where each view ray meets the cloud layer, one unit above the viewer.
cx = dx[mask] / py
cz = dz[mask] / py
sx, sz = P["stretch"]
nx = cx * P["scale"] / sx
nz = cz * P["scale"] / sz
# One pixel spans 2pi/W radians; on a layer one unit up that is about (1 + distance squared) times as much layer.
footprint = (1 + cx * cx + cz * cz) * (2 * math.pi / W) * P["scale"] * max(1 / sx, 1 / sz)
density = fbm(nx, nz, P["octaves"], footprint)
if "soft" in P:
    # The homepage's clouds: white wherever the noise rises past a threshold, rising steeply enough to form puffs.
    gain, offset = P["soft"]
    c = np.clip(gain * density - offset, 0, 1)
    cloud_col = np.ones((len(density), 3), dtype=np.float32)
else:
    cover = P["coverage"]
    c = np.clip((density - (1 - cover)) / (cover * 0.55), 0, 1)
    c = c * c * (3 - 2 * c)
    # Light the clouds: brighter where the density falls off towards the light.
    off = 0.08
    ldx, ldz = light_direction[0], light_direction[2]
    ln = math.hypot(ldx, ldz) or 1
    density_lit = fbm(nx + ldx / ln * off, nz + ldz / ln * off, P["octaves"], footprint)
    light = np.clip(0.55 + (density - density_lit) * 6, 0, 1)
    lit, shadow = hexrgb(P["lit"]), hexrgb(P["shadow"])
    cloud_col = shadow + (lit - shadow) * light[:, None]
# Far away the clouds melt into the horizon haze.
dist = np.hypot(cx, cz)
fade = np.exp(-dist / 9.0)
alpha = (c * fade * 0.97)[:, None]
sky_part = col[mask]
if "under_layer" in P:
    # A second layer of soft clouds in its own colour, drawn under the main ones from a shifted patch of the same noise,
    # so its clouds fall in different places.
    under = P["under_layer"]
    gain, offset = under["soft"]
    shift_x, shift_z = under["shift"]
    under_density = fbm(nx + shift_x, nz + shift_z, P["octaves"], footprint)
    under_alpha = (np.clip(gain * under_density - offset, 0, 1) * fade * 0.97)[:, None]
    sky_part = sky_part * (1 - under_alpha) + hexrgb(under["colour"]) * under_alpha
col[mask] = sky_part * (1 - alpha) + cloud_col * alpha

# A soft band of haze right on the horizon hides where the cloud layer and the ground meet.
band = np.exp(-(dy / 0.035) ** 2)[..., None] * 0.55
col = col * (1 - band) + hor * band

img = np.clip(col, 0, 1)
# Tiny dither to avoid banding in the gradients.
img = np.clip(img + (rng.random(img.shape, dtype=np.float32) - 0.5) / 255, 0, 1)

import bpy
os.makedirs(OUT, exist_ok=True)
image = bpy.data.images.new(PRESET, W, H, alpha=False)
rgba = np.concatenate([img, np.ones((H, W, 1), dtype=np.float32)], axis=2)
image.pixels.foreach_set(rgba[::-1].astype(np.float32).ravel())
image.filepath_raw = os.path.join(OUT, f"{PRESET}.png")
image.file_format = "PNG"
image.save()
print("saved", image.filepath_raw)
