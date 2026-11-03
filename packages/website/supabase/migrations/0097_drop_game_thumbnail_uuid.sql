-- The site shows ai_thumbnail_uuid, or screenshot_uuid when a game has no AI thumbnail, so nothing reads or writes thumbnail_uuid.
alter table public.game_metadata drop column thumbnail_uuid;
