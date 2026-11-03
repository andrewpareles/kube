import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readSoundData } from '../packages/website/app/assets/[asset_id]/sound/readSoundData.ts'

test('sound preview removes only a valid engine wrapper', () => {
    const ogg = new TextEncoder().encode('OggS test audio')
    const asset = new Uint8Array(16 + ogg.byteLength)
    const view = new DataView(asset.buffer)
    view.setBigUint64(0, BigInt(1), true)
    view.setBigUint64(8, BigInt(ogg.byteLength), true)
    asset.set(ogg, 16)
    assert.deepEqual(readSoundData(asset), ogg)
    const padded = new Uint8Array(asset.byteLength + 12)
    padded.set(asset, 5)
    assert.deepEqual(readSoundData(padded.subarray(5, 5 + asset.byteLength)), ogg)
    assert.throws(() => readSoundData(asset.slice(0, 15)))
    assert.throws(() => readSoundData(asset.slice(0, -1)))
    const invalidOgg = asset.slice()
    invalidOgg[16] = 0
    assert.throws(() => readSoundData(invalidOgg), /Invalid Ogg/)
    view.setBigUint64(0, BigInt(2), true)
    assert.throws(() => readSoundData(asset), /Unsupported/)
})
