import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const gameDetailPageSource = fs.readFileSync(repoPath("packages/website/app/games/[game_id]/GameDetailPage.tsx"), "utf8");
const gameDetailRouteSource = fs.readFileSync(repoPath("packages/website/app/games/[game_id]/page.tsx"), "utf8");
const gameSettingsDialogSource = fs.readFileSync(repoPath("packages/website/app/games/[game_id]/GameSettingsDialog.tsx"), "utf8");
const gameSettingsFieldsSource = fs.readFileSync(repoPath("packages/website/components/site/GameSettingsFields.tsx"), "utf8");
const gameSettingsFormSource = fs.readFileSync(repoPath("packages/website/lib/gameSettingsForm.ts"), "utf8");
// The settings form is the dialog plus the fields it shares with the new game dialog, so the
// assertions about what the form shows are made against both files together.
const gameSettingsFormMarkup = gameSettingsDialogSource + gameSettingsFieldsSource;
const deleteGameDialogSource = fs.readFileSync(repoPath("packages/website/app/games/[game_id]/DeleteGameDialog.tsx"), "utf8");
const alertDialogSource = fs.readFileSync(repoPath("packages/website/shadcn/components/ui/alert-dialog.tsx"), "utf8");
const siteGamesSource = fs.readFileSync(repoPath("packages/website/lib/siteGames.ts"), "utf8");
const edgegapActionsSource = fs.readFileSync(repoPath("packages/website/app/server-actions/edgegap-actions.ts"), "utf8");
const gameSettingsActionsSource = fs.readFileSync(repoPath("packages/website/app/server-actions/game-settings.ts"), "utf8");
const gameSettingsLibSource = fs.readFileSync(repoPath("packages/website/lib/gameSettings.ts"), "utf8");
const editorGameSettingsRoutePath = repoPath("packages/website/app/api/editor/games/[gameId]/settings/route.ts");
const browserEditorRouteSource = fs.readFileSync(repoPath("packages/website/app/editor/[game_id]/page.tsx"), "utf8");
const editorAccessSource = fs.readFileSync(repoPath("packages/website/lib/editorAccess.ts"), "utf8");

test("game detail only shows edit for the owner", () => {
  assert.match(siteGamesSource, /creatorId: games\.creatorId/);
  assert.match(gameDetailRouteSource, /getGameDetailIfAllowed\(gameId\)/);
  assert.match(gameDetailRouteSource, /isOwner=\{game\.isOwner\}/);
  assert.match(gameDetailPageSource, /isOwner: boolean/);
  assert.match(gameDetailPageSource, /\{isOwner && \(/);
  assert.match(gameDetailPageSource, /href=\{EDITOR_PATH\(game\.gameId\)\}/);
  assert.match(gameDetailPageSource, /<Pencil className="size-4" \/>\s*Edit/);
  assert.ok(gameDetailPageSource.indexOf("href={EDITOR_PATH(game.gameId)}") > gameDetailPageSource.indexOf("onClick={() => handleLaunch({ requestId: null })}"));
});

test("publish actions update owner games and revalidate affected pages", () => {
  assert.match(edgegapActionsSource, /export const unpublishGame = async \(gameId: number, inviteToken: string \| null\) => \{/);
  assert.match(edgegapActionsSource, /const game = await getGameIfOwned\(\{ gameId, creatorId: editor\.creatorId \}\)/);
  assert.match(edgegapActionsSource, /isPublished: false/);
  assert.match(edgegapActionsSource, /return \{ isPublished: false \}/);
  assert.match(edgegapActionsSource, /return \{ isPublished: true \}/);
  assert.match(edgegapActionsSource, /revalidatePath\(`\/games\/\$\{gameId\}`\)/);
  assert.match(edgegapActionsSource, /revalidatePath\(`\/profile\/\$\{creatorUserId\}`\)/);
  assert.match(edgegapActionsSource, /revalidatePath\("\/"\)/);
  assert.match(edgegapActionsSource, /revalidatePath\("\/create"\)/);
  assert.equal(edgegapActionsSource.includes('revalidatePath("/games")'), false);
});

test("owner settings dialog edits game metadata without backdrop dismissal", () => {
  assert.match(gameSettingsDialogSource, /import \{\s*Dialog,\s*DialogContent,\s*DialogFooter,\s*DialogHeader,\s*DialogTitle,\s*\} from '@\/shadcn\/components\/ui\/dialog'/);
  assert.equal(gameSettingsDialogSource.includes("DialogDescription"), false);
  assert.match(gameSettingsDialogSource, /onInteractOutside=\{\(event\) => event\.preventDefault\(\)\}/);
  assert.match(gameSettingsDialogSource, /onOpenAutoFocus=\{\(event\) => event\.preventDefault\(\)\}/);
  assert.match(gameSettingsDialogSource, /max-h-\[calc\(100dvh-2rem\)\]/);
  assert.match(gameSettingsDialogSource, /overflow-y-auto/);
  assert.match(gameSettingsDialogSource, /overscroll-contain/);
  assert.equal(gameSettingsDialogSource.includes("border-b"), false);
  assert.match(gameSettingsDialogSource, /className="border-t border-gray-100 pt-5"/);
  assert.equal(gameSettingsDialogSource.includes("divide-y"), false);
  assert.equal(gameSettingsFormMarkup.includes("border-gray-200"), false);
  assert.equal(gameSettingsFormMarkup.includes("border-red-200"), false);
  assert.match(gameSettingsDialogSource, /<DialogTitle>Game Settings<\/DialogTitle>/);
  assert.equal(gameSettingsDialogSource.includes("Details, visibility, and player count."), false);
  assert.match(gameSettingsFormMarkup, /<label htmlFor="game-settings-name"/);
  assert.match(gameSettingsFormMarkup, /<label htmlFor="game-settings-description"/);
  assert.match(gameSettingsFormMarkup, /<label htmlFor="game-settings-max-players"/);
  assert.match(gameSettingsFormMarkup, /Max 250 characters\./);
  assert.match(gameSettingsFormMarkup, /maxLength=\{250\}/);
  assert.match(gameSettingsFieldsSource, /max=\{MAX_PLAYERS_LIMIT\}/);
  assert.match(gameSettingsFormSource, /MAX_PLAYERS_LIMIT = 40/);
  assert.match(gameSettingsFormSource, /Max players must be between 1 and \$\{MAX_PLAYERS_LIMIT\}\./);
  assert.match(gameSettingsFormMarkup, /<Switch\s*checked=\{isPublished\}/);
  assert.match(gameSettingsFormMarkup, /isPublished \? 'Published' : 'Private'/);
  assert.match(gameSettingsFormMarkup, /Visible on our Games page\./);
  assert.match(gameSettingsFormMarkup, /Only you can view\./);
  assert.equal(gameSettingsFormMarkup.includes("Public."), false);
  assert.equal(gameSettingsFormMarkup.includes("Private."), false);
  assert.equal(gameSettingsFormMarkup.includes("Shown anywhere this game appears."), false);
  assert.equal(gameSettingsFormMarkup.includes("A short note for the game page."), false);
  assert.equal(gameSettingsFormMarkup.includes("Used when new servers start."), false);
  assert.equal(gameSettingsFormMarkup.includes("Published games can be played."), false);
  assert.equal(gameSettingsFormMarkup.includes("Only you can open it."), false);
  assert.match(gameSettingsDialogSource, /<span className="text-sm font-medium text-gray-950">Danger Zone<\/span>/);
  assert.match(gameSettingsDialogSource, /<Button type="button" variant="secondary" onClick=\{\(\) => onOpenChange\(false\)\} disabled=\{isSaving\}>Cancel<\/Button>/);
  assert.match(gameSettingsDialogSource, /\{isSaving \? 'Saving\.\.\.' : 'Save'\}/);
  assert.equal(gameSettingsDialogSource.includes("<Check"), false);
  assert.match(gameSettingsActionsSource, /export const updateGameSettings = async/);
  assert.match(gameSettingsActionsSource, /updateGameSettingsForCreator/);
  assert.match(gameSettingsLibSource, /name: trimmedName/);
  assert.match(gameSettingsLibSource, /description: trimmedDescription \|\| null/);
  assert.match(gameSettingsLibSource, /maxPlayers,/);
  assert.match(gameSettingsLibSource, /isPublished,/);
  // Saving settings does not go near the game's worlds.
  const updateGameSettingsSource = gameSettingsLibSource.slice(
    gameSettingsLibSource.indexOf("export const updateGameSettingsForCreator"),
    gameSettingsLibSource.indexOf("export const deleteGameForCreator"),
  );
  assert.equal(updateGameSettingsSource.includes("worldVersion"), false);
  assert.equal(updateGameSettingsSource.includes("latestGameworld"), false);
  assert.equal(updateGameSettingsSource.includes("orderBy(desc(gameWorlds.versionNum))"), false);
});

test("website settings stay on the website helper instead of an editor api bridge", () => {
  assert.equal(fs.existsSync(editorGameSettingsRoutePath), false);
  assert.match(gameSettingsActionsSource, /updateGameSettingsForCreator/);
  assert.match(gameSettingsLibSource, /revalidatePath\(`\/games\/\$\{gameId\}`\)/);
  assert.match(gameSettingsLibSource, /revalidatePath\("\/create"\)/);
  assert.equal(gameSettingsLibSource.includes("worldVersion"), false);
});

test("owner delete dialog removes game-owned rows", () => {
  assert.match(gameSettingsDialogSource, /import \{ DeleteGameDialog \} from '\.\/DeleteGameDialog'/);
  assert.match(gameSettingsDialogSource, /const \[isDangerZoneOpen, setIsDangerZoneOpen\] = useState\(false\)/);
  assert.match(gameSettingsDialogSource, /const \[isDeleteDialogOpen, setIsDeleteDialogOpen\] = useState\(false\)/);
  assert.match(gameSettingsDialogSource, /setIsDangerZoneOpen\(!isDangerZoneOpen\)/);
  assert.equal(gameSettingsDialogSource.includes("Permanent actions for this game."), false);
  assert.equal(gameSettingsDialogSource.includes("Delete this game permanently."), false);
  assert.match(gameSettingsDialogSource, /Irreversible\. Consider changing Visibility instead\./);
  assert.match(gameSettingsDialogSource, /variant="destructive"\s*onClick=\{\(\) => setIsDeleteDialogOpen\(true\)\}/);
  assert.match(gameSettingsDialogSource, /<DeleteGameDialog\s*gameId=\{game\.gameId\}\s*gameName=\{game\.name\}\s*isOpen=\{isDeleteDialogOpen\}/);
  assert.match(deleteGameDialogSource, /AlertDialog/);
  assert.match(deleteGameDialogSource, /const handleOverlayClick = \(\) => \{/);
  assert.match(deleteGameDialogSource, /if \(isDeleting\) \{\s*return\s*\}/);
  assert.match(deleteGameDialogSource, /onOpenChange\(false\)/);
  assert.match(deleteGameDialogSource, /onOverlayClick=\{handleOverlayClick\}/);
  assert.equal(deleteGameDialogSource.includes("event.preventDefault()"), false);
  assert.match(alertDialogSource, /onOverlayClick\?: React\.MouseEventHandler<HTMLDivElement>/);
  assert.match(alertDialogSource, /<AlertDialogOverlay onClick=\{onOverlayClick\} \/>/);
  assert.match(deleteGameDialogSource, /<AlertDialogTitle>Delete \{gameName\}\?<\/AlertDialogTitle>/);
  assert.match(deleteGameDialogSource, /This removes the game, its likes and visits, and its edit history\. This cannot be undone\./);
  assert.match(deleteGameDialogSource, /variant="destructive"/);
  assert.match(deleteGameDialogSource, /router\.push\(profilePath\)/);
  assert.match(gameSettingsActionsSource, /export const deleteGame = async \(gameId: number\) => \{/);
  assert.match(gameSettingsActionsSource, /deleteGameForCreator/);
  assert.match(gameSettingsLibSource, /\.delete\(startingServers\)/);
  assert.match(gameSettingsLibSource, /\.delete\(runningServers\)/);
  assert.match(gameSettingsLibSource, /\.delete\(userLikes\)/);
  assert.match(gameSettingsLibSource, /\.delete\(games\)/);
  assert.match(gameSettingsLibSource, /\.delete\(gameWorlds\)/);
  assert.match(gameSettingsLibSource, /profilePath: `\/profile\/\$\{game\.creatorUserId\}`/);
});

test("game detail fetch allows owners to see private games", () => {
  assert.match(siteGamesSource, /export const getGameDetailIfAllowed = async \(gameId: number\)/);
  assert.match(siteGamesSource, /const isOwner = user\?\.id === game\.creatorId/);
  assert.match(siteGamesSource, /if \(\(!isPublished \|\| !game\.hasWorld\) && !isOwner\) \{/);
  assert.match(siteGamesSource, /isPublished,/);
  assert.match(siteGamesSource, /isOwner,/);
});

test("editing opens the owned browser workspace without preparing desktop authorization", () => {
  assert.match(gameDetailPageSource, /href=\{EDITOR_PATH\(game\.gameId\)\}/);
  assert.equal(gameDetailPageSource.includes("useGameEditLaunch"), false);
  assert.equal(gameDetailPageSource.includes('clicked="edit"'), false);
  assert.match(browserEditorRouteSource, /getLoginUrl\(\{ next: EDITOR_PATH\(gameId\) \}\)/);
  assert.match(browserEditorRouteSource, /const game = await getEditorAccess\(gameId, user\?\.id \?\? null, inviteToken\)/);
  assert.match(editorAccessSource, /g\.creator_id::text=\$\{profileId\}/);
  assert.equal(browserEditorRouteSource.includes("prepareEditorLaunch"), false);
});

test("play waits for a saved world, edit does not", () => {
  // A game with no world has nothing to start, and only its own creator can reach its page at all.
  // Edit is what gets them a world, so it is never held back.
  assert.match(siteGamesSource, /hasWorld: boolean/);
  assert.match(siteGamesSource, /hasWorld: game\.hasWorld,/);
  assert.match(gameDetailRouteSource, /hasWorld: game\.hasWorld,/);
  assert.match(gameDetailPageSource, /const shouldDisablePlay = isLaunchPending \|\| !isPublished \|\| !game\.hasWorld/);
  assert.match(gameDetailPageSource, /playLabel = 'Publish from the editor to play'/);
  assert.equal(/disabled=\{isEditLaunchPending \|\| !game\.hasWorld\}/.test(gameDetailPageSource), false);

  // One note at a time: the world has to come before visibility can matter.
  assert.match(gameDetailPageSource, /\{!game\.hasWorld && \(/);
  assert.match(gameDetailPageSource, /\{!isPublished && game\.hasWorld && \(/);
});

test("a game with no saved world is only its own creator's to see", () => {
  // Making a game writes no world, so without this a game made in the browser and left alone would
  // sit on the games page with a play button that has nothing to start.
  assert.match(siteGamesSource, /const hasWorld = sql<boolean>`exists \(select 1 from \$\{gameWorlds\} where \$\{gameWorlds\.gameId\} = \$\{games\.gameId\}\)`/);
  // Top and recent both ask for it, and so does someone else's profile.
  assert.equal(siteGamesSource.split(".where(and(eq(games.isPublished, true), hasWorld))").length - 1, 2);
  assert.match(siteGamesSource, /eq\(games\.creatorId, profile\.id\),\s*eq\(games\.isPublished, true\),\s*hasWorld,/);

  // Its own creator still sees it in their profile and on its own page, so they can get to the
  // editor and give it one.
  assert.match(siteGamesSource, /const allowedGamesWhere = isOwner\s*\? eq\(games\.creatorId, profile\.id\)/);
  assert.match(siteGamesSource, /if \(\(!isPublished \|\| !game\.hasWorld\) && !isOwner\) \{\s*return null/);

  // Your own games list is the one place a draft with no world still belongs.
  const myGamesSource = siteGamesSource.slice(
    siteGamesSource.indexOf("export const getMyGames"),
    siteGamesSource.indexOf("export const getAllowedProfileGames"),
  );
  assert.equal(myGamesSource.includes("hasWorld"), false);
});
