import { test } from 'node:test'
import assert from 'node:assert/strict'

Object.assign(process.env, {
    EDITOR_SERVER_SECRET: 'test-only-secret-that-is-at-least-32-characters',
    ARBITRIUM_REQUEST_ID: 'this-deployment',
    GAME_ID: '42',
    EDITOR_SERVER_PORT: '8080',
    EDITOR_CHAT_PORT: '8081',
    SITE_URL: 'https://website.example',
    SUPABASE_URL: 'https://supabase.invalid',
    SUPABASE_SECRET_KEY: 'test-only',
    S3_ACCESS_KEY_ID: 'test-only',
    S3_SECRET_ACCESS_KEY: 'test-only',
    AWS_REGION: 'us-east-1',
    AWS_BEARER_TOKEN_BEDROCK: 'test-only',
    AZURE_OPENAI_API_KEY: 'test-only',
})
const { convertDataPart } = await import('../src/helpers/chat.ts')
test('Selected objects reach the model as their own block, keyed the way the tools name ids', () => {
    const part = convertDataPart({
        type: 'data-selected-objects',
        data: [
            { objectId: 12, name: 'Red Car', kind: 'RigidGroup' },
            { objectId: 3, name: 'Lamp', kind: 'Ball' },
        ],
    })
    assert.deepEqual(part, {
        type: 'text',
        text: '<selected_objects>\nThe user had these objects selected in the editor when sending this message:\n{"object_id":12,"name":"Red Car","kind":"RigidGroup"}\n{"object_id":3,"name":"Lamp","kind":"Ball"}\n</selected_objects>',
    })
})

test('The turn object list and malformed attachments never reach the model', () => {
    assert.equal(convertDataPart({ type: 'data-object-changes', data: [] }), undefined)
    assert.equal(convertDataPart({ type: 'data-selected-objects', data: [{ objectId: 'x' }] }), undefined)
})
