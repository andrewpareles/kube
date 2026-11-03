import { z } from 'zod'

const portSchema = z.object({
    external: z.number().nullable(),
    protocol: z.string(),
    link: z.string().nullable(),
    tls_upgrade: z.boolean().nullable(),
})
export const editorDeploymentStatusSchema = z.object({
    request_id: z.string(),
    running: z.boolean(),
    error: z.boolean(),
    current_status: z.string(),
    fqdn: z.string().nullable(),
    public_ip: z.string().nullable(),
    ports: z.record(z.string(), portSchema).nullable(),
})
// Both browser APIs need TLS, and desktop playtests need the deployment's UDP port.
export const readEditorConnectionInfo = (status: z.infer<typeof editorDeploymentStatusSchema>) => {
    const editorPort = status.ports?.editor
    const chatPort = status.ports?.chat
    const gamePort = status.ports?.gameport
    if (
        !status.running ||
        !status.public_ip ||
        !status.fqdn ||
        !editorPort?.external ||
        !chatPort?.external ||
        !gamePort?.external
    )
        return
    if (
        !editorPort.tls_upgrade ||
        !chatPort.tls_upgrade ||
        !gamePort.protocol.toUpperCase().includes('UDP')
    )
        throw new Error('Editor Edgegap ports are misconfigured.')
    const editorUrl = `https://${status.fqdn}:${editorPort.external}`
    // Return both live addresses from Edgegap; chat needs no duplicate hosting state in the database.
    return {
        request_id: status.request_id,
        editor_url: editorUrl,
        public_ip: status.public_ip,
        playtest_external_port: gamePort.external,
        chat_url: `https://${status.fqdn}:${chatPort.external}`,
    }
}
