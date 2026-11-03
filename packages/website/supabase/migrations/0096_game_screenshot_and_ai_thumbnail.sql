-- Publishing uploads a screenshot, and then the AI's redrawn version of it. thumbnail_uuid is the one the site shows: the
-- screenshot until the AI's version is ready.
alter table public.game_metadata add column screenshot_uuid uuid unique;
alter table public.game_metadata add column ai_thumbnail_uuid uuid unique;
update public.game_metadata set screenshot_uuid = thumbnail_uuid;
