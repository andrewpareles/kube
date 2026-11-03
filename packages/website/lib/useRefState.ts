'use client'

import { useCallback, useRef, useState } from 'react'

// Keep the value in a ref and request a render whenever the setter is called.
export const useRefState = <T,>(initVal: T) => {
    const [, _setState] = useState(0)
    const ref = useRef(initVal)

    const setState = useCallback((newVal: T) => {
        ref.current = newVal
        // Setting null again would leave React's state unchanged and skip the render.
        _setState((renderCount) => renderCount + 1)
    }, [])

    return [ref, setState] as const
}
