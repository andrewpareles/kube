# Sound library summary

Updated October 2, 2026. This is the single retained record of the music and sound-effect imports. Local audio files and separate batch records were removed after verifying the uploads.

## Uploaded music

35 sound assets are stored at `data/sound_data/{data_id}` in the public S3 bucket `kube-new-public`, served from cdn.kube.new, and their rows are in Supabase project `cackpkffqqxhvaqdsnkp`. Asset names contain song titles; artist credits and source links are retained below. The user confirmed download and reuse permission for the imports.

| Sound ID | Asset name | Artist / credit | Duration | Source |
|---|---|---|---|---|
| 1 | Armageddon (Short Version) | LeaF | 2:27 | [YouTube](https://www.youtube.com/watch?v=O3E2epPs1Js) |
| 2 | Verlesq (Short Version) | LeaF | 1:58 | [YouTube](https://www.youtube.com/watch?v=_9OZk3sk2_Q) |
| 3 | もぺもぺ | LeaF | 1:52 | [YouTube](https://www.youtube.com/watch?v=nC-bVtpIMd4) |
| 4 | Lazer Grrl Main Music | Not supplied | 1:02 | [YouTube](https://www.youtube.com/watch?v=mF4QpHtoKjM) |
| 5 | Flying Soda (Long Version) | 猫叉Master feat. 林ももこ | 5:55 | [YouTube](https://www.youtube.com/watch?v=7hCD_TH-tBo) |
| 6 | I'd Rather Sleep | Kero Kero Bonito | 1:56 | [YouTube](https://www.youtube.com/watch?v=c4j9Z9bwBBU) |
| 7 | Funk do Bounce | Ariis | 1:41 | [YouTube](https://www.youtube.com/watch?v=kWW-jZ6H_LE) |
| 8 | Blue Blood | Heinz Kiessling | 2:54 | [YouTube](https://www.youtube.com/watch?v=wQ3Az55eqhQ) |
| 9 | All My Fellas | Frizk | 2:21 | [YouTube](https://www.youtube.com/watch?v=-ol3PSROlwg) |
| 10 | Trainer Red Epic Remix | DJTheFishhead | 5:27 | [YouTube](https://www.youtube.com/watch?v=buifCKc9RNU) |
| 11 | Rog Unlimitation | Not supplied | 2:59 | [YouTube](https://www.youtube.com/watch?v=p0vqWi0nb0c) |
| 12 | Them Changes | Thundercat | 3:08 | [YouTube](https://www.youtube.com/watch?v=BuzJ5NArvgw) |
| 13 | Rog Limitation | Not supplied | 2:59 | [YouTube](https://www.youtube.com/watch?v=1A44lG8CJfA) |
| 14 | Turn It Around | Sub Focus feat. Kele | 3:31 | [YouTube](https://www.youtube.com/watch?v=QhsIgnOi7hQ) |
| 15 | Discord Stage Waiting Music | Not supplied | 15:05 | [YouTube](https://www.youtube.com/watch?v=_XiTkF8aP3U) |
| 16 | Let's Fly Away, Pt. 2 | JIKES feat. Nori | 4:55 | [YouTube](https://www.youtube.com/watch?v=y5QaCpZhgzs) |
| 17 | Angels We Have Heard on High | Caribbean Sound | 6:19 | [YouTube](https://www.youtube.com/watch?v=ZcO-eawZ00I) |
| 18 | Trap Royalty | very cool tutorials | 1:13 | [YouTube](https://www.youtube.com/watch?v=I6clX1LknkU) |
| 19 | Over the Fullereneshift | DJ Sharpnel | 4:30 | [YouTube](https://www.youtube.com/watch?v=rN2mtNvDVGw) |
| 20 | Monastery in Disguise | Retro Game Memories; Never Forget Me | 3:36 | [YouTube](https://www.youtube.com/watch?v=Cgl8jnoEUak) |
| 21 | Giant Steps (Mono) | John Coltrane | 4:48 | [YouTube](https://www.youtube.com/watch?v=g8QnKWPYRCs) |
| 22 | CORE | Toby Fox | 2:51 | [YouTube](https://www.youtube.com/watch?v=SEOYyM9n9mw) |
| 23 | Gritty Shaker | David Holmes | 6:40 | [YouTube](https://www.youtube.com/watch?v=HMuk9Qtzjgk) |
| 24 | Song That Might Play When You Fight Sans | Toby Fox | 1:02 | [YouTube](https://www.youtube.com/watch?v=47GvY9rOMUs) |
| 25 | Title Wave | boggio, Crablooshi | 1:56 | [YouTube](https://www.youtube.com/watch?v=Yk8Q1-lLtfI) |
| 26 | Void Star | RundownSD, NOLI, RUNDOWN | 2:35 | [YouTube](https://www.youtube.com/watch?v=bqIr3ZvZjR8) |
| 27 | Death by Glamour | Toby Fox | 2:15 | [YouTube](https://www.youtube.com/watch?v=Q9kDr4na0ls) |
| 28 | Picnic Rose | Vasco, Patrick West | 2:54 | [YouTube](https://www.youtube.com/watch?v=EqQsFQl6e-c) |
| 29 | Tropical Jazz | Djkriss19, Christopher Ortiz | 2:07 | [YouTube](https://www.youtube.com/watch?v=c7XjtP9YcEw) |
| 30 | Hoedown | Aaron Copland | 3:26 | [YouTube](https://www.youtube.com/watch?v=LsReWx9XdNs) |
| 31 | Squall | 猫叉Master+ | 2:02 | [YouTube](https://www.youtube.com/watch?v=5ul9VBr14RI) |
| 32 | Mr. Lucky | Karl Jenkins, De Wolfe Music | 3:33 | [YouTube](https://www.youtube.com/watch?v=3auP_y6BA5Q) |
| 33 | Ghost Assassin | Maduk feat. Veela | 3:42 | [YouTube](https://www.youtube.com/watch?v=tEcggRukZCs) |
| 34 | 625 Sandwich Stacker - In-Game Music (Extended) | Not supplied | 30:00 | [YouTube](https://www.youtube.com/watch?v=KiZa9xcV8zI) |
| 35 | Hoedown | Béla Fleck and the Flecktones | 4:55 | [YouTube](https://www.youtube.com/watch?v=NwWmR1pZT1E) |

All 35 current assets passed full public download, wrapper parsing, audio decoding, duration/non-silence checks, and game playback advancement and clock-pause checks. Before deleting local files, their asset names and storage byte counts were checked against the live Supabase records.

ID 30 is Aaron Copland’s **Hoedown**, which replaced the full Rodeo recording. ID 35 is **Hoedown** by Béla Fleck and the Flecktones. Funk do Bounce was imported once as ID 7. The 625 Sandwich Stacker recording is the full 30-minute extended version.

`data_metadata` contains `id`, `name`, `data_type`, `data_id`, `uploaded_by`, and `created_at`. Paths are derived from the type and ID; there is no `url` column. Stored audio uses the engine’s versioned `SoundData` format containing Ogg Vorbis, rather than MP3.

## Uploaded sound effects

22 sound effects were uploaded on October 2, 2026 as Sound assets published by user 17 (Kubester), with `created_at` set to September 29, 2026 on both the asset and sound data rows. Andrew picked and named them by ear from 386 CC0 candidates gathered from 7 Kenney packs and 18 OpenGameArt packs. Each clip had leading silence trimmed and was otherwise uploaded at its original level, without normalizing. Every source is CC0; [bosslevelaudio's Weird Whooshes](https://opengameart.org/content/wierd-whooshes) offers several licenses and was not used.

| Asset ID | Sound data ID | Name | Source file | Pack |
|---|---|---|---|---|
| 239 | 39 | Swoosh 1 | swish-1.wav | [artisticdude — Swishes Sound Pack](https://opengameart.org/content/swishes-sound-pack) |
| 240 | 40 | Swoosh 2 | swish-2.wav | [artisticdude — Swishes Sound Pack](https://opengameart.org/content/swishes-sound-pack) |
| 241 | 41 | Swoosh 3 | swish-4.wav | [artisticdude — Swishes Sound Pack](https://opengameart.org/content/swishes-sound-pack) |
| 242 | 42 | Swoosh 4 | swish-5.wav | [artisticdude — Swishes Sound Pack](https://opengameart.org/content/swishes-sound-pack) |
| 243 | 43 | Swoosh 5 | swish-6.wav | [artisticdude — Swishes Sound Pack](https://opengameart.org/content/swishes-sound-pack) |
| 244 | 44 | Swoosh 6 | swish-7.wav | [artisticdude — Swishes Sound Pack](https://opengameart.org/content/swishes-sound-pack) |
| 245 | 45 | Soft footstep 1 | footstep_grass_003.ogg | [Kenney — Impact Sounds](https://kenney.nl/assets/impact-sounds) |
| 246 | 46 | Soft footstep 2 | footstep_concrete_000.ogg | [Kenney — Impact Sounds](https://kenney.nl/assets/impact-sounds) |
| 247 | 47 | Hard footstep | footstep05.ogg | [Kenney — RPG Audio](https://kenney.nl/assets/rpg-audio) |
| 248 | 48 | Cutlass swing 1 | swing.wav | [artisticdude — RPG Sound Pack](https://opengameart.org/content/rpg-sound-pack) |
| 249 | 49 | Cutlass swing 2 | swing2.wav | [artisticdude — RPG Sound Pack](https://opengameart.org/content/rpg-sound-pack) |
| 250 | 50 | Cutlass swing 3 | swing3.wav | [artisticdude — RPG Sound Pack](https://opengameart.org/content/rpg-sound-pack) |
| 251 | 51 | Short boom | bang_02.ogg | [rubberduck — 25 CC0 bang / firework SFX](https://opengameart.org/content/25-cc0-bang-firework-sfx) |
| 252 | 52 | Huge explosion | Chunky Explosion.mp3 | [joth — Chunky Explosion](https://opengameart.org/content/chunky-explosion) |
| 253 | 53 | Faint explosion in distance | lowFrequency_explosion_000.ogg | [Kenney — Sci-fi Sounds](https://kenney.nl/assets/sci-fi-sounds) |
| 254 | 54 | Retro hit | sfx_damage_hit3.wav | [Juhani Junkala — 512 Sound Effects (8-bit)](https://opengameart.org/content/512-sound-effects-8-bit-style) |
| 255 | 55 | Bubbles | sfx_sounds_falling1.wav | [Juhani Junkala — 512 Sound Effects (8-bit)](https://opengameart.org/content/512-sound-effects-8-bit-style) |
| 256 | 56 | Stomp 1 | Jump 1.wav | [macro — Jump Landing](https://opengameart.org/content/jump-landing) |
| 257 | 57 | Stomp 2 | Jump 2.wav | [macro — Jump Landing](https://opengameart.org/content/jump-landing) |
| 258 | 58 | Stomp 3 | Jump 3.wav | [macro — Jump Landing](https://opengameart.org/content/jump-landing) |
| 259 | 59 | Computery access confirmed | confirmation_001.ogg | [Kenney — Interface Sounds](https://kenney.nl/assets/interface-sounds) |
| 260 | 60 | Card flipped from deck | card-place-1.ogg | [Kenney — Casino Audio](https://kenney.nl/assets/casino-audio) |

They were published with an ignored `editor_server` test modeled on `publish_default_skyboxes.rs`, which ran each file through the same conversion as browser uploads. The candidates, the picker page and the test were deleted afterward.

### Sources

Use CC0 assets for the built-in library. Kube.new lets creators reuse assets across their own games, so the license must permit redistribution as well as use in a finished game. [CC0](https://creativecommons.org/publicdomain/zero/1.0/) permits copying, modification, distribution, and commercial use without asking permission. Keep the original author, asset page, source filename, and sound data ID with each selection.

Avoid using the general label “royalty-free” as permission to populate a reusable library. For example, [Pixabay's license summary](https://pixabay.com/service/license-summary/) prohibits standalone redistribution.

### Engine findings

Default materials currently map friendly names to existing texture data IDs. A default-sound tool can use the same pattern: map a name such as `jump` to an uploaded sound data ID, create a `SoundPlayer` and paused `Clock`, and return their object IDs. Game scripts then trigger the clock. Merely adding a sound to assets should not load it in every game.

The texture and sound paths both use `DataFetcherArc`, deduplicate downloads by asset kind and ID, cache compressed assets, and decode in background workers. Textures then become GPU textures; sound becomes stereo floating-point PCM shared by playback handles.

The current sound manager begins fetching and decoding only when a referenced clock is playing. A cold, short effect may finish on the clock before its audio becomes ready. Load referenced effects before their first trigger, with explicit readiness where exact first playback matters.

Music currently uses full-file download and full decode, not streaming. Three minutes at 44,100 stereo frames per second and two four-byte samples per frame uses 63,504,000 bytes of decoded samples, plus compressed audio and other overhead. Kira offers [incremental decoding for streaming sounds](https://docs.rs/kira/latest/kira/sound/streaming/index.html); adopting it for music is a separate runtime change and does not by itself provide network streaming.

### Upload workflow

The existing editor uploader accepts WAV, MP3, FLAC, and OGG, including folders and nested folders. It converts sources to Ogg Vorbis, wraps them in `SoundData`, uploads them through the API, and displays each sound data ID. Uploading alone does not register an AI default or add a `SoundPlayer` to the world.

Dev mode already supplies the configured development editor credential. Its API URL points at `http://localhost:10000`, so the local API must be running. The uploader tab is currently disabled by `SHOULD_SHOW_DATA_UPLOADER_TAB` in `crates/shared/src/game/consts.rs`; it must be enabled to use the editor UI. Dev uploads go to the configured real Supabase project.

The music and sound-effect imports are listed above.
