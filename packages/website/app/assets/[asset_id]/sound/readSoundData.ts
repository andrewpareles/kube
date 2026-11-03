// Keep this in sync with SOUND_DATA_CURRENT_VERSION in crates/game_types/src/consts.rs.
const SOUND_DATA_CURRENT_VERSION = 1

// Sound assets wrap Ogg bytes with a bincode version and byte count. Validate both before serving audio.
export const readSoundData = (bytes: Uint8Array) => {
    if (bytes.byteLength < 16) throw new Error('Truncated sound asset')
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
    if (view.getBigUint64(0, true) !== BigInt(SOUND_DATA_CURRENT_VERSION)) throw new Error('Unsupported sound asset version')
    if (view.getBigUint64(8, true) !== BigInt(bytes.byteLength - 16)) throw new Error('Invalid sound asset length')
    const ogg = bytes.slice(16)
    if (new TextDecoder().decode(ogg.slice(0, 4)) !== 'OggS') throw new Error('Invalid Ogg sound')
    return ogg
}
