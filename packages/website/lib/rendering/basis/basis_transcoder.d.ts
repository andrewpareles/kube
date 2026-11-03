// The Basis Universal API used by asset texture previews.
declare const initializeBasis: (options: { wasmBinary: Uint8Array }) => Promise<{
    initializeBasis: () => void
    KTX2File: new (bytes: Uint8Array) => {
        startTranscoding: () => number
        getImageTranscodedSizeInBytes: (level: number, layer: number, face: number, format: number) => number
        transcodeImage: (pixels: Uint8Array, level: number, layer: number, face: number, format: number, flags: number, channel0: number, channel1: number) => number
        close: () => void
        delete: () => void
    }
}>

export = initializeBasis
