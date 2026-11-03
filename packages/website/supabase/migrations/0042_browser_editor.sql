-- The saved editor world is independent from published versions. Deployment rows track live
-- editor processes in the same starting/running arrangement as production game servers.
create table public.game_editor_worlds (
    game_id bigint primary key references public.game_metadata(game_id) on delete cascade,
    gameworld_json text not null,
    updated_at timestamptz not null default now()
);
create table public.starting_editor_servers (
    request_id text primary key,
    game_id bigint not null unique references public.game_metadata(game_id) on delete cascade,
    starting_at timestamptz not null default now()
);
create table public.running_editor_servers (
    request_id text primary key,
    game_id bigint not null unique references public.game_metadata(game_id) on delete cascade,
    editor_url text not null,
    public_ip text not null,
    playtest_external_port integer not null check (playtest_external_port between 1 and 65535)
);
alter table public.game_editor_worlds enable row level security;
alter table public.starting_editor_servers enable row level security;
alter table public.running_editor_servers enable row level security;
create policy "Service role only" on public.game_editor_worlds for all to service_role using (true) with check (true);
create policy "Service role only" on public.starting_editor_servers for all to service_role using (true) with check (true);
create policy "Service role only" on public.running_editor_servers for all to service_role using (true) with check (true);

-- AI SDK messages retain their stable ids inside the existing JSON payload. Old messages are
-- kept for history; their format is explicit so they can be displayed without being submitted
-- to AI SDK as invalid model input.
alter table public.chat_messages add column message_format text not null default 'legacy';
alter table public.chat_metadata add column turn_status text not null default 'idle'
    check (turn_status in ('idle', 'responding', 'interrupted'));
-- No desktop editor credential is issued by the browser editor.
alter table public.player_auth drop column edit_auth_hash;
