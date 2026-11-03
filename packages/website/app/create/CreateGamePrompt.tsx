'use client'

import { useEffect, useState, type KeyboardEvent, type RefObject } from 'react'
import { ArrowUpRight } from 'lucide-react'

const EXAMPLE_PROMPTS = [
    'freeze tag with powerups...',
    'a mix of Subway Surfers and Flappy Bird...',
    'forest hide and seek...',
    'a zombies vs humans game with guns...',
    'a jungle game where you find loot...',
    'a medieval fighting game...',
    'a 4v4 competitive shooter...',
    'a cotton candy factory...',
]

// TODO: AI-PICKED-VALUE: The example types every 30 milliseconds, holds for 1.4 seconds and
// deletes every 42 milliseconds, which reads naturally without ever delaying the person typing.
const EXAMPLE_TYPE_MS = 30
const EXAMPLE_DELETE_MS = 42
const EXAMPLE_HOLD_MS = 1400
const EXAMPLE_RESTART_MS = 450
const EXAMPLE_START_MS = 350

/**
 * Types each example prompt into the placeholder, deletes it, and starts the next one, so the empty
 * box suggests what to write. It runs whenever the box is empty, including after someone clears what
 * they wrote, and it never runs at all for someone who has asked for reduced motion.
 */
const useTypedExamplePlaceholder = (isStopped: boolean) => {
    const [typedExample, setTypedExample] = useState('')

    useEffect(() => {
        if (isStopped) return

        let characterCount = 0
        let isDeleting = false
        let examplePromptIndex = 0
        let timeoutId = 0

        const typeNextCharacter = () => {
            const examplePrompt = EXAMPLE_PROMPTS[examplePromptIndex]
            characterCount += isDeleting ? -1 : 1
            setTypedExample(examplePrompt.slice(0, characterCount))

            if (!isDeleting && characterCount === examplePrompt.length) {
                isDeleting = true
                timeoutId = window.setTimeout(typeNextCharacter, EXAMPLE_HOLD_MS)
                return
            }
            if (isDeleting && characterCount === 0) {
                isDeleting = false
                // Backspacing one example to nothing is what moves it along to the next, and the
                // last one leads back to the first, so the box keeps suggesting things forever.
                examplePromptIndex = (examplePromptIndex + 1) % EXAMPLE_PROMPTS.length
                timeoutId = window.setTimeout(typeNextCharacter, EXAMPLE_RESTART_MS)
                return
            }
            timeoutId = window.setTimeout(typeNextCharacter, isDeleting ? EXAMPLE_DELETE_MS : EXAMPLE_TYPE_MS)
        }

        // Someone who has asked for reduced motion gets one whole example at once instead.
        const showWholeExample = () => setTypedExample(EXAMPLE_PROMPTS[0])
        const shouldReduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
        timeoutId = window.setTimeout(shouldReduceMotion ? showWholeExample : typeNextCharacter, EXAMPLE_START_MS)

        // Wiping the example on the way out is what makes it start over cleanly. Otherwise
        // whatever it had reached would sit there waiting, and clearing the box would flash the
        // tail end of the last pass before the new one began.
        return () => {
            window.clearTimeout(timeoutId)
            setTypedExample('')
        }
    }, [isStopped])

    return typedExample
}

type CreateGamePromptProps = {
    input: string
    isSubmitting: boolean
    textareaRef: RefObject<HTMLTextAreaElement | null>
    onInputChange: (value: string) => void
    onSubmit: () => Promise<void>
}

export function CreateGamePrompt({
    input,
    isSubmitting,
    textareaRef,
    onInputChange,
    onSubmit,
}: CreateGamePromptProps) {
    // The example is only ever hidden behind what the person has written, so clearing the box
    // brings it back.
    const typedExample = useTypedExamplePlaceholder(input.length > 0)
    // Reveal the submit button for typed, pasted, or restored text; placeholder examples do not count.
    // Once revealed, keep the button visible until this component is remounted.
    const [isCreateButtonVisible, setIsCreateButtonVisible] = useState(input.length > 0)
    if (!isCreateButtonVisible && input.length > 0) {
        setIsCreateButtonVisible(true)
    }

    const handleTextareaKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
        if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
            event.preventDefault()
            void onSubmit()
        }
    }

    // The box is filled a hair off the white page, so the one thing you can write in is set apart
    // from it without being a block of grey. The line around it is drawn at full strength whether
    // or not it has the cursor, because it is the one thing on the page asking to be written in.
    return (
        <div className="create-composer-group">
            {/* A click anywhere in the box that is not on its button puts the cursor in the text, padding included. */}
            <form
                className="create-composer"
                onMouseDown={(event) => {
                    if (event.target instanceof Element && (event.target.closest('button') || event.target === textareaRef.current)) return
                    event.preventDefault()
                    textareaRef.current?.focus()
                }}
                onSubmit={async (event) => {
                    event.preventDefault()
                    await onSubmit()
                }}
            >
                {/* Row 1: text input */}
                <div className="px-5 pt-5">
                    <label className="sr-only" htmlFor="game-prompt">Describe your game</label>
                    <textarea
                        id="game-prompt"
                        ref={textareaRef}
                        value={input}
                        onChange={(event) => onInputChange(event.target.value)}
                        onKeyDown={handleTextareaKeyDown}
                        placeholder={`I want to make ${typedExample}`}
                        style={{ maxHeight: 150 }}
                        className="text-foreground placeholder:text-gray-400"
                    />
                </div>

                {/* Row 2: action buttons. Its padding is the row above's less the padding inside
                    the buttons themselves, which is what puts the icons on the text's own line. */}
                <div className="flex items-center justify-end px-[15px] pb-[13px] pt-[5px]">
                    {/* Left: attachments, image */}

                    {/* Right: mic, send */}
                    <div className="flex items-center gap-2">
                        <button
                            type="submit"
                            disabled={!input.trim() || isSubmitting}
                            aria-label="Start building"
                            aria-hidden={!isCreateButtonVisible}
                            className="site-button"
                        >
                            {isSubmitting ? 'Opening Editor...' : 'Start Building'}
                            <ArrowUpRight className="size-4" aria-hidden="true" />
                        </button>
                    </div>
                </div>
            </form>
        </div>
    )
}

// Earlier animated placeholder notes are retained; the approved composer uses a static hint.
// These examples fill the working prompt so they can be edited before submission.
// TODO: AI-PICKED-VALUE: The box stops growing at six lines, which is about as tall as it can get
// before it starts crowding the tagline and mark above it. 28px is the line height Tailwind gives
// text-lg, so the cap always lands on a whole line rather than halfway through one.
