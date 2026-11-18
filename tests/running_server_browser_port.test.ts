import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'
import { repoPath } from './testPaths.ts'

const migrationSource = fs.readFileSync(repoPath('packages/website/supabase/migrations/0099_running_server_webtransport_port.sql'), 'utf8')
const journalSource = fs.readFileSync(repoPath('packages/website/supabase/migrations/meta/_journal.json'), 'utf8')
const schemaSource = fs.readFileSync(repoPath('packages/website/supabase/migrations/schema.ts'), 'utf8')
const serverTrackingSource = fs.readFileSync(repoPath('packages/website/lib/serverTracking.ts'), 'utf8')
const edgegapApiSource = fs.readFileSync(repoPath('packages/website/lib/edgegapApi.ts'), 'utf8')
const connectionInfoRouteSource = fs.readFileSync(repoPath('packages/website/app/api/play-sessions/[requestId]/connection-info/route.ts'), 'utf8')
const playSceneSource = fs.readFileSync(repoPath('packages/website/app/play/[game_id]/PlayScene.tsx'), 'utf8')

// A browser port is only known once Edgegap publishes it, and a deployment that never does must leave the server
// joinable from the desktop client rather than recording a port that answers nothing.
test('a running server records the browser port Edgegap published, and may have none', () => {
    assert.match(migrationSource, /alter table public\.running_servers add column webtransport_port integer;/)
    assert.equal(/webtransport_port integer not null/.test(migrationSource), false)
    assert.match(schemaSource, /webtransportPort: integer\("webtransport_port"\)/)
    assert.equal(/webtransportPort: integer\("webtransport_port"\)\.notNull\(\)/.test(schemaSource), false)
    assert.match(journalSource, /"tag": "0099_running_server_webtransport_port"/)
    assert.equal(journalSource.indexOf('0099_running_server_webtransport_port') > journalSource.indexOf('0098_revoke_browser_table_rights'), true)
})

// Promotion is the one place a starting server becomes joinable, so both of its writes carry the browser port.
test('promotion writes the browser port beside the QUIC address', () => {
    assert.match(serverTrackingSource, /webtransportPort: number \| null;/)
    assert.match(serverTrackingSource, /insert\(runningServers\)[\s\S]*?externalPort,\s*webtransportPort,\s*\}\)/)
    assert.match(serverTrackingSource, /onConflictDoUpdate\(\{[\s\S]*?externalPort,\s*webtransportPort,\s*\},/)
})

// The ready webhook is where the deployment's ports are read, so it must pass both of them on.
test('the ready webhook promotes with the browser port it read from the deployment', () => {
    assert.match(edgegapApiSource, /webtransportPort: connectionInfo\.webtransportPort,/)
    assert.match(edgegapApiSource, /webtransportPort: parseEdgegapUdpPort\(\{\s*ports: deploymentStatus\.ports,\s*portName: EDGEGAP_WEBTRANSPORT_PORT_NAME,/)
})

// A server that is already running used to answer with the QUIC address alone, which is what left a page unable to join.
test('a running server answers the page with its browser address', () => {
    assert.match(connectionInfoRouteSource, /webtransportPort: runningServers\.webtransportPort,/)
    assert.match(connectionInfoRouteSource, /webtransport: runningWebtransport\.webtransport,/)
    assert.match(connectionInfoRouteSource, /webtransportPort: runningServer\.webtransportPort,/)
})

// A site with no host suffix cannot name a server under the certificate's wildcard, and says so instead of answering
// with an address the browser would refuse.
test('a site that cannot name a server answers with why, and the page shows it', () => {
    assert.match(edgegapApiSource, /if \(!hostSuffix\) \{\s*throw new Error\(\s*"This site has no WEBTRANSPORT_HOST_SUFFIX, so it cannot name the servers a browser joins\."/)
    assert.match(connectionInfoRouteSource, /return NextResponse\.json\(\s*\{ error: runningWebtransport\.error \},\s*\{ status: 500 \},\s*\)/)
    assert.match(connectionInfoRouteSource, /return NextResponse\.json\(\s*\{ error: startingWebtransport\.error \},\s*\{ status: 500 \},\s*\)/)
    assert.match(playSceneSource, /throw new Error\(body\.error \?\? 'This game could not be joined\.'\)/)
})
