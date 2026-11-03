// The chat stream uses newline-delimited JSON; chunks can split lines or UTF-8 characters.
export const readEditorEvents = async function* (response: Response): AsyncGenerator<unknown> {
    if (!response.body) throw new Error('The editor returned no event stream.')
    const reader = response.body.pipeThrough(new TextDecoderStream()).getReader()
    let pending = ''
    try {
        while (true) {
            const { value, done } = await reader.read()
            if (done) break
            pending += value
            let newline: number
            while ((newline = pending.indexOf('\n')) !== -1) {
                const line = pending.slice(0, newline); pending = pending.slice(newline + 1)
                if (line.trim()) yield JSON.parse(line)
            }
        }
        if (pending.trim()) yield JSON.parse(pending)
    } finally {
        try { await reader.cancel() } finally { reader.releaseLock() }
    }
}
