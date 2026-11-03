// A subscriber receives a snapshot followed by events published while that snapshot was loading.
// This is live fan-out only: reconnecting asks for a new snapshot instead of replaying old chunks.
export const createEventBroadcast = () => {
    const listeners = new Set<{ receive: (line: string) => void; close: () => void }>()
    const encoder = new TextEncoder()
    const publish = (event: unknown) => {
        const line = JSON.stringify(event) + '\n'
        for (const listener of listeners) listener.receive(line)
    }
    // End the current subscriptions after their queued messages; new subscribers can join the next turn.
    const closeSubscribers = () => {
        for (const listener of listeners) listener.close()
    }
    const subscribe = (snapshot: () => Promise<unknown>, signal: AbortSignal) => {
        let cleanup = () => {}
        return new ReadableStream<Uint8Array>(
            {
                start(stream) {
                    let pending: string[] | null = []
                    let isClosed = false
                    let isClosing = false
                    cleanup = () => {
                        isClosed = true
                        listeners.delete(listener)
                        signal.removeEventListener('abort', abort)
                    }
                    const abort = () => {
                        cleanup()
                        stream.close()
                    }
                    const receive = (line: string) => {
                        // TODO: AI-PICKED-VALUE: Disconnect clients more than 640 events behind instead of retaining an unbounded queue.
                        if ((pending?.length ?? 0) >= 640 || (stream.desiredSize ?? 0) <= 0) {
                            cleanup()
                            stream.error(new Error('Editor chat fell behind. Reconnect.'))
                            return
                        }
                        if (pending) pending.push(line)
                        else stream.enqueue(encoder.encode(line))
                    }
                    const listener = {
                        receive,
                        close: () => {
                            // Finish this snapshot and its queued events without collecting the next turn.
                            listeners.delete(listener)
                            if (pending) isClosing = true
                            else abort()
                        },
                    }
                    if (signal.aborted) {
                        abort()
                        return
                    }
                    listeners.add(listener)
                    signal.addEventListener('abort', abort, { once: true })
                    void snapshot()
                        .then((state) => {
                            if (isClosed) return
                            stream.enqueue(encoder.encode(JSON.stringify(state) + '\n'))
                            const buffered = pending ?? []
                            pending = null
                            for (const line of buffered) {
                                if (!isClosed) receive(line)
                            }
                            if (isClosing && !isClosed) abort()
                        })
                        .catch((error) => {
                            if (!isClosed) {
                                cleanup()
                                stream.error(error)
                            }
                        })
                },
                cancel() {
                    cleanup()
                },
            },
            { highWaterMark: 640 },
        )
    }
    return { publish, subscribe, closeSubscribers }
}
