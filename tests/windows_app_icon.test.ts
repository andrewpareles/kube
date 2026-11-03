import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import zlib from "node:zlib";
import { repoPath } from "./testPaths.ts";

// Windows reads an app icon out of the executable and out of the installer, not out of a bundle the
// way macOS does, so the icon only shows up if both of these point at the checked-in .ico that
// the generate-icons script writes into the Windows packaging assets. Losing either one puts the
// default grey Windows program icon back, which is the bug these tests exist for.
const buildScriptSource = fs.readFileSync(repoPath("crates/client/build.rs"), "utf8");
const windowsPackagerSource = fs.readFileSync(repoPath("packaging/client/Packager.windows.toml"), "utf8");
const windowsIconFile = fs.readFileSync(repoPath("packaging/client/assets/windows/icon.ico"));

/** The sizes the generator draws. 16 and 24 are the ones a task bar and Explorer actually show. */
const expectedIconSides = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256];

/**
 * The images inside a .ico. The file is a six byte header, then one sixteen byte record per image,
 * then the images themselves. A width byte of zero means 256, which is why it is read that way.
 */
const readIconEntries = (icoFile: Buffer) => {
    const entryCount = icoFile.readUInt16LE(4);
    const entries: { side: number; image: Buffer }[] = [];
    for (let entryIndex = 0; entryIndex < entryCount; entryIndex++) {
        const recordOffset = 6 + entryIndex * 16;
        const side = icoFile.readUInt8(recordOffset) || 256;
        const imageLength = icoFile.readUInt32LE(recordOffset + 8);
        const imageOffset = icoFile.readUInt32LE(recordOffset + 12);
        entries.push({ side, image: icoFile.subarray(imageOffset, imageOffset + imageLength) });
    }
    return entries;
};

/**
 * The size, the colour type and the top left pixel of a PNG.
 *
 * The top left pixel is what says whether the icon has a background behind the mark. It can be read
 * without undoing the whole PNG filtering, because the first pixel of the first row has no pixel to
 * its left and no row above it, so every one of the five filters leaves it as its own raw bytes.
 */
const readPngHeaderAndFirstPixel = (pngImage: Buffer) => {
    assert.deepEqual(pngImage.subarray(0, 8), Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), "icon entry is not a PNG");
    const width = pngImage.readUInt32BE(16);
    const height = pngImage.readUInt32BE(20);
    const colourType = pngImage.readUInt8(25);
    const interlaceMethod = pngImage.readUInt8(28);

    // Walk the chunks for the pixel data, which can be split across several IDATs.
    const imageDataParts: Buffer[] = [];
    let chunkOffset = 8;
    while (chunkOffset < pngImage.length) {
        const chunkLength = pngImage.readUInt32BE(chunkOffset);
        const chunkType = pngImage.subarray(chunkOffset + 4, chunkOffset + 8).toString("ascii");
        if (chunkType === "IDAT") imageDataParts.push(pngImage.subarray(chunkOffset + 8, chunkOffset + 8 + chunkLength));
        if (chunkType === "IEND") break;
        chunkOffset += chunkLength + 12;
    }
    const pixelData = zlib.inflateSync(Buffer.concat(imageDataParts));
    const topLeftPixel = { red: pixelData.readUInt8(1), green: pixelData.readUInt8(2), blue: pixelData.readUInt8(3), alpha: pixelData.readUInt8(4) };

    return { width, height, colourType, interlaceMethod, topLeftPixel };
};

test("the Windows icon holds a drawing of its own for every size a task bar asks for", () => {
    const entries = readIconEntries(windowsIconFile);
    assert.deepEqual(entries.map(({ side }) => side), expectedIconSides);

    for (const { side, image } of entries) {
        const { width, height, interlaceMethod } = readPngHeaderAndFirstPixel(image);
        // Each size is drawn at its size rather than scaled down from the biggest one, which is what
        // keeps the K's diagonals readable at 16 and 24 pixels.
        assert.equal(width, side, `the ${side} pixel entry holds a ${width} pixel image`);
        assert.equal(height, side, `the ${side} pixel entry holds a ${height} pixel image`);
        assert.equal(interlaceMethod, 0, `the ${side} pixel entry is interlaced`);
    }
});

test("the Windows icon has no background behind the mark", () => {
    for (const { side, image } of readIconEntries(windowsIconFile)) {
        const { colourType, topLeftPixel } = readPngHeaderAndFirstPixel(image);
        assert.equal(colourType, 6, `the ${side} pixel entry has no alpha channel`);
        // The macOS icon sits on a white tile because a dock crops it to a rounded square. Nothing
        // on Windows does, so this one is the mark on nothing and its corners are see-through.
        assert.equal(topLeftPixel.alpha, 0, `the ${side} pixel entry has a background in its corner`);
    }
});

test("everything that shows a Windows app icon points at the generated one", () => {
    // Explorer, the task bar, the Start menu, the shortcuts the installer writes and the
    // Add/Remove Programs entry all read this out of client.exe itself.
    assert.match(buildScriptSource, /packaging\/client\/assets\/windows\/icon\.ico/);

    assert.match(windowsPackagerSource, /^icons = \["assets\/windows\/icon\.ico"\]$/m);
    assert.match(windowsPackagerSource, /^installer-icon = "assets\/windows\/icon\.ico"$/m);
});
