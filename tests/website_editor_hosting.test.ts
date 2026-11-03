import assert from 'node:assert/strict'
import test from 'node:test'
import {
    editorDeploymentStatusSchema,
    readEditorConnectionInfo,
} from '../packages/website/lib/editorServerConnection.ts'

const readyDeployment = {
    request_id: 'editor-deployment',
    running: true,
    error: false,
    current_status: 'Status.READY',
    fqdn: 'editor.example',
    public_ip: '192.0.2.1',
    ports: {
        editor: { external: 31000, protocol: 'WS', link: null, tls_upgrade: true },
        chat: { external: 31001, protocol: 'WS', link: null, tls_upgrade: true },
        gameport: { external: 31002, protocol: 'UDP', link: null, tls_upgrade: false },
    },
}

test('editor connection info includes both browser APIs and the desktop playtest address', () => {
    const status = editorDeploymentStatusSchema.parse(readyDeployment)
    assert.deepEqual(readEditorConnectionInfo(status), {
        request_id: 'editor-deployment',
        editor_url: 'https://editor.example:31000',
        chat_url: 'https://editor.example:31001',
        public_ip: '192.0.2.1',
        playtest_external_port: 31002,
    })
})

test('starting deployments and incomplete routing are not advertised as connectable', () => {
    for (const status of [
        { ...readyDeployment, running: false },
        { ...readyDeployment, public_ip: null },
        { ...readyDeployment, fqdn: null },
        { ...readyDeployment, ports: null },
        {
            ...readyDeployment,
            ports: {
                ...readyDeployment.ports,
                chat: { ...readyDeployment.ports.chat, external: null },
            },
        },
        {
            ...readyDeployment,
            ports: { editor: readyDeployment.ports.editor, chat: readyDeployment.ports.chat },
        },
    ]) {
        assert.equal(
            readEditorConnectionInfo(editorDeploymentStatusSchema.parse(status)),
            undefined,
        )
    }
})

test('browser ports require TLS and desktop playtests require UDP', () => {
    for (const ports of [
        {
            ...readyDeployment.ports,
            editor: { ...readyDeployment.ports.editor, tls_upgrade: false },
        },
        { ...readyDeployment.ports, chat: { ...readyDeployment.ports.chat, tls_upgrade: false } },
        {
            ...readyDeployment.ports,
            gameport: { ...readyDeployment.ports.gameport, protocol: 'TCP' },
        },
    ]) {
        assert.throws(
            () =>
                readEditorConnectionInfo(
                    editorDeploymentStatusSchema.parse({ ...readyDeployment, ports }),
                ),
            /misconfigured/,
        )
    }
})

test('malformed deployment responses fail validation before promotion', () => {
    assert.equal(
        editorDeploymentStatusSchema.safeParse({ ...readyDeployment, request_id: undefined })
            .success,
        false,
    )
    assert.equal(
        editorDeploymentStatusSchema.safeParse({
            ...readyDeployment,
            ports: { editor: { external: '31000' } },
        }).success,
        false,
    )
})
