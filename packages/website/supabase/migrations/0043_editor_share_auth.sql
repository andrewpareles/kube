-- Invitations belong to a game, not a running editor. No row or a null token means sharing is off.
create table public.editor_share_auth (
    game_id bigint primary key references public.game_metadata(game_id) on delete cascade,
    invite_token text
);
alter table public.editor_share_auth enable row level security;
create policy "Service role only" on public.editor_share_auth for all to service_role using (true) with check (true);
