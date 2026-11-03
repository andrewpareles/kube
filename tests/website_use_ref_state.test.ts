import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test, { type TestContext } from 'node:test'
import { useRefState } from '../packages/website/lib/useRefState.ts'

const require = createRequire(new URL('../packages/website/package.json', import.meta.url))
const { act, createElement, StrictMode, useLayoutEffect } = require('react')
const { createRoot } = require('react-dom/client')
const { JSDOM } = require('jsdom')

type RefStateResult<T> = ReturnType<typeof useRefState<T>>

// Exercise the hook with React's real renderer, including Strict Mode and committed effects.
const RefStateProbe = <T,>({ initialValue, onRender }: {
    initialValue: T
    onRender: (state: RefStateResult<T>) => void
}) => {
    const state = useRefState(initialValue)
    useLayoutEffect(() => { onRender(state) })
    const [ref] = state
    return createElement('output', null, String(ref.current))
}

const renderRefState = async <T,>(t: TestContext, initialValue: T) => {
    const dom = new JSDOM('<!doctype html><html><body></body></html>')
    Object.assign(globalThis, { window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true })
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)
    const renders: RefStateResult<T>[] = []
    const onRender = (state: RefStateResult<T>) => { renders.push(state) }
    t.after(async () => {
        await act(async () => { root.unmount() })
        dom.window.close()
        Reflect.deleteProperty(globalThis, 'window')
        Reflect.deleteProperty(globalThis, 'document')
        Reflect.deleteProperty(globalThis, 'IS_REACT_ACT_ENVIRONMENT')
    })
    const render = async (initialValue: T) => {
        await act(async () => {
            root.render(createElement(StrictMode, null, createElement(RefStateProbe<T>, { initialValue, onRender })))
        })
    }
    await render(initialValue)
    return {
        container,
        renders,
        render,
        read: () => {
            const state = renders.at(-1)
            assert.ok(state)
            return state
        },
    }
}

test('useRefState updates immediately, rerenders, and keeps the ref and setter stable', async (t) => {
    const rendered = await renderRefState(t, 0)
    const [ref, setState] = rendered.read()
    assert.equal(rendered.container.textContent, '0')

    await act(async () => {
        setState(1)
        assert.equal(ref.current, 1)
        assert.equal(rendered.container.textContent, '0')
    })
    const [updatedRef, updatedSetter] = rendered.read()
    assert.equal(updatedRef, ref)
    assert.equal(updatedSetter, setState)
    assert.equal(rendered.container.textContent, '1')

    await rendered.render(99)
    assert.equal(ref.current, 1)
    assert.equal(rendered.read()[0], ref)
    assert.equal(rendered.read()[1], setState)
})

test('multiple updates and callbacks from older renders read the current value', async (t) => {
    const rendered = await renderRefState(t, 0)
    const [ref, setState] = rendered.read()
    await act(async () => {
        setState(ref.current + 1)
        setState(ref.current + 1)
        assert.equal(ref.current, 2)
    })
    await act(async () => {
        await Promise.resolve()
        setState(ref.current + 1)
        assert.equal(ref.current, 3)
    })
    assert.equal(rendered.container.textContent, '3')
})

test('a submission guard stops repeated calls before a render and can be released', async (t) => {
    const rendered = await renderRefState(t, false)
    const [isSubmitting, setIsSubmitting] = rendered.read()
    let submissions = 0
    const submit = () => {
        if (isSubmitting.current) return
        setIsSubmitting(true)
        submissions += 1
    }
    await act(async () => {
        submit()
        submit()
        assert.equal(submissions, 1)
    })
    assert.equal(rendered.container.textContent, 'true')
    await act(async () => { setIsSubmitting(false) })
    assert.equal(rendered.container.textContent, 'false')
    await act(async () => { submit() })
    assert.equal(submissions, 2)
})

test('function values are stored directly without being called', async (t) => {
    let calls = 0
    const initialFunction = () => { calls += 1; return 'initial' }
    const nextFunction = () => { calls += 1; return 'next' }
    const rendered = await renderRefState(t, initialFunction)
    const [ref, setState] = rendered.read()
    assert.equal(ref.current, initialFunction)

    await act(async () => { setState(nextFunction) })
    assert.equal(ref.current, nextFunction)
    assert.equal(calls, 0)
})

test('calling the setter requests a render even when the value is unchanged', async (t) => {
    const rendered = await renderRefState(t, null)
    const [ref, setState] = rendered.read()
    const renderCount = rendered.renders.length
    await act(async () => { setState(null) })
    assert.equal(ref.current, null)
    assert.ok(rendered.renders.length > renderCount)
})
