import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'
import { repoPath } from './testPaths.ts'

const editorRouteSource = fs.readFileSync(repoPath('packages/website/app/editor/[game_id]/page.tsx'), 'utf8')
const createPageSource = fs.readFileSync(repoPath('packages/website/app/create/CreatePage.tsx'), 'utf8')
const newGameSource = fs.readFileSync(repoPath('packages/website/app/profile/[user_id]/NewGameDialog.tsx'), 'utf8')

// A public game must not become editable just because someone knows its URL.
test('browser editor requires access before rendering and shares its authorization check with startup', () => {
    assert.match(editorRouteSource, /if \(!Number\.isSafeInteger\(gameId\) \|\| gameId <= 0\) notFound\(\)/)
    assert.match(editorRouteSource, /if \(user === null && inviteToken === null\) redirect\(getLoginUrl\(\{ next: EDITOR_PATH\(gameId\) \}\)\)/)
    assert.match(editorRouteSource, /await getEditorAccess\(gameId, user\?\.id \?\? null, inviteToken\)/)
    const startupSource = fs.readFileSync(repoPath('packages/website/app/api/edgegap-editor/start-editor-server/[gameId]/route.ts'), 'utf8')
    assert.match(startupSource, /await getEditorAccess\(gameId, user\?\.id \?\? null, inviteToken\)/)
    assert.ok(startupSource.indexOf('if (!game)') < startupSource.indexOf('await openEditorServer('))
    assert.match(editorRouteSource, /if \(!game\) notFound\(\)/)
    assert.ok(editorRouteSource.indexOf('if (!game) notFound()') < editorRouteSource.indexOf('return <BrowserEditor'))
    assert.match(editorRouteSource, /robots: \{ index: false, follow: false \}/)
    assert.equal(/createGame|prepareEditorLaunch|playerAuth|\.insert\(|\.update\(/.test(editorRouteSource), false)
})

// Both creation entry points keep the existing game limits and session-derived creator identity.
test('website creation uses existing actions and opens the resulting game in the browser', () => {
    for (const source of [createPageSource, newGameSource]) {
        assert.match(source, /await createGame\(\{/)
        assert.match(source, /router\.push\(EDITOR_PATH\(gameId\)\)/)
        assert.equal(/sendDeepLink|prepareEditorLaunch|saveRecentSubmitPrompt|useDeeplinkFeedback/.test(source), false)
    }
    assert.match(createPageSource, /if \(!input\.trim\(\) \|\| isSubmitting\.current\) return/)
    assert.ok(createPageSource.indexOf('if (user === null)') < createPageSource.indexOf('await createGame('))
    assert.match(createPageSource, /isPublished: false/)
})

test('one pending prompt is saved only for login and cleared after successful creation', () => {
    assert.match(createPageSource, /if \(user === null\) \{\s*sessionStorage\.setItem\(EDITOR_PROMPT_DRAFT_STORAGE_KEY, input\)/)
    assert.ok(createPageSource.indexOf('sessionStorage.setItem(EDITOR_PROMPT_DRAFT_STORAGE_KEY, input)') < createPageSource.indexOf('openLoginModal('))
    assert.match(createPageSource, /sessionStorage\.getItem\(EDITOR_PROMPT_DRAFT_STORAGE_KEY\)/)
    assert.match(createPageSource, /sessionStorage\.removeItem\(EDITOR_PROMPT_DRAFT_STORAGE_KEY\)/)
    assert.ok(createPageSource.indexOf('sessionStorage.removeItem(EDITOR_PROMPT_DRAFT_STORAGE_KEY)') > createPageSource.indexOf('await createGame('))
    assert.equal(createPageSource.match(/sessionStorage\.setItem\(/g)?.length, 1)
    assert.equal(createPageSource.includes('editorPromptStorageKey'), false)
    assert.match(createPageSource, /openLoginModal\(\{ next: CREATE_PATH \}\)/)
    assert.equal(/encodeURIComponent\(input\)|URLSearchParams/.test(createPageSource), false)
})
