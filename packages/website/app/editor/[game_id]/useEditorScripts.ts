'use client'

import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import type { ScriptTypeEnvironment, ScriptServices } from '../editorScripts'
import type { EditorWorldState } from '../editorWorld'

// Script text streams to the server as it is typed, so this only requests declarations.
// The server supplies types when scripts open; Monaco's local worker handles checks while typing.
export const useEditorScripts = (state: EditorWorldState, services: ScriptServices) => {
    const hasOpenScripts = state.openScriptIds.length > 0
    const [types, setTypes] = useState<{ services: ScriptServices, environment: ScriptTypeEnvironment | null } | null>(null)
    useEffect(() => {
        if (!hasOpenScripts) return
        const controller = new AbortController()
        void services.readTypes(controller.signal).then(environment => {
            if (!controller.signal.aborted) { setTypes({ services, environment }); toast.dismiss('editor-script-types-error') }
        }).catch(error => {
            if (controller.signal.aborted) return
            setTypes({ services, environment: null })
            toast.error('Could not load script types', { description: error instanceof Error ? error.message : String(error), id: 'editor-script-types-error' })
        })
        return () => controller.abort()
    }, [hasOpenScripts, services])
    return { typeEnvironment: types?.services === services ? types.environment : null, isLoadingTypes: hasOpenScripts && types?.services !== services }
}
