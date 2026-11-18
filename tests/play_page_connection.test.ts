import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'
import { repoPath } from './testPaths.ts'

const playPageSource = fs.readFileSync(repoPath('packages/website/app/play/[game_id]/page.tsx'), 'utf8')
const playSceneSource = fs.readFileSync(repoPath('packages/website/app/play/[game_id]/PlayScene.tsx'), 'utf8')
const connectionInfoRouteSource = fs.readFileSync(repoPath('packages/website/app/api/play-sessions/[requestId]/connection-info/route.ts'), 'utf8')
const edgegapApiSource = fs.readFileSync(repoPath('packages/website/lib/edgegapApi.ts'), 'utf8')

// A page reserves its seat before it renders, and a bad id never reaches the reservation.
test('the play page reserves a seat and passes the browser the token to join with', () => {
    assert.ok(playPageSource.indexOf('if (!/^\\d+$/.test(gameIdParam)) notFound()') < playPageSource.indexOf('await getJoinGameInfoAnyGame('))
    assert.match(playPageSource, /<PlayScene requestId=\{joinInfo\.requestId\} playAuth=\{joinInfo\.playAuth\} \/>/)
    assert.match(playPageSource, /referrer: 'no-referrer'/)
})

// The page waits for a starting server, dials it by name over HTTP/3, and accepts the server's certificate by its hash.
test('the play scene dials the game server over webtransport once the site reports it ready', () => {
    assert.match(playSceneSource, /fetch\(`\/api\/play-sessions\/\$\{requestId\}\/connection-info`/)
    assert.ok(playSceneSource.indexOf('if (response.status === 202)') < playSceneSource.indexOf('connectionInfo.webtransport === null'))
    assert.match(playSceneSource, /window\.atob\(certificate_sha256_base64\)/)
    assert.match(playSceneSource, /serverUrl: `https:\/\/\$\{host\}:\$\{port\}\/play`/)
    assert.match(playSceneSource, /await playWeb\.default\(\)/)
    assert.match(playSceneSource, /await playWeb\.BrowserPlaySession\.connect\(canvas, serverUrl, playAuth, certificateHash\)/)
    assert.match(playSceneSource, /session\.render_frame\(nowMs\)/)
    assert.match(playSceneSource, /sessionRef\.current\?\.set_key\(event\.code, isDown\)/)
    assert.match(playSceneSource, /sessionRef\.current\?\.turn_camera\(event\.movementX, event\.movementY\)/)
    assert.equal(/localStorage|sessionStorage/.test(playSceneSource), false)
})

// The site leaves the browser address out until the deployment publishes the browser's port, so the page keeps polling
// instead of dialing an address that does not exist yet.
test('connection info reports the browser address only once the deployment publishes it', () => {
    assert.match(connectionInfoRouteSource, /if \(!connectionInfo\) \{\s*return NextResponse\.json\(\{\}, \{ status: 202 \}\)/)
    assert.match(connectionInfoRouteSource, /webtransport: readEdgegapWebtransportInfo\(\{ requestId, webtransportPort \}\)/)
    assert.ok(connectionInfoRouteSource.indexOf('webtransportPort: runningServer.webtransportPort') > 0)
    assert.ok(connectionInfoRouteSource.indexOf('{ status: 202 }') < connectionInfoRouteSource.indexOf('promoteStartingServerToRunning('))
})

// Edgegap publishes the browser port under its own name, and the page needs the deployment's address for it.
test('the browser port is read by name from the deployment and addressed under its own zone', () => {
    assert.match(edgegapApiSource, /EDGEGAP_WEBTRANSPORT_PORT_NAME = "webtransport"/)
    assert.match(edgegapApiSource, /parseEdgegapUdpPort\(\{\s*ports: deploymentStatus\.ports,\s*portName: EDGEGAP_WEBTRANSPORT_PORT_NAME,\s*edgegapInternalPort: getEdgegapWebtransportInternalPort\(\),/)
    assert.match(edgegapApiSource, /`\$\{requestId\}\.\$\{hostSuffix\}`/)
    assert.match(edgegapApiSource, /process\.env\.WEBTRANSPORT_HOST_SUFFIX\?\.trim\(\)/)
    assert.match(edgegapApiSource, /certificate_sha256_base64:/)
})
