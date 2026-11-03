import assert from 'node:assert/strict'
import test from 'node:test'
import { getGameSettingsFormError, newGameFormValues, NEW_GAME_DEFAULT_NAME } from '../packages/website/lib/gameSettingsForm.ts'

test('manual creation opens with an empty title and resets it when reopened', () => {
    const values = newGameFormValues()
    assert.equal(values.name, '')
    assert.equal(getGameSettingsFormError(values), 'Game title is required.')

    values.name = 'Island race'
    assert.equal(getGameSettingsFormError(values), null)
    assert.equal(newGameFormValues().name, '')
    assert.equal(values.name, 'Island race')
    assert.ok(NEW_GAME_DEFAULT_NAME.trim().length > 0, 'automatic editor creation still needs a default name')
})

test('only a nonblank title satisfies the required-title check', () => {
    const values = newGameFormValues()
    for (const name of ['', '   ', '\t\n']) {
        assert.equal(getGameSettingsFormError({ ...values, name }), 'Game title is required.')
    }
    assert.equal(getGameSettingsFormError({ ...values, name: '  Island race  ' }), null)
})
