create table public.game_bookmarks (
  game_id bigint not null
    references public."games-metadata" (game_id)
    on delete cascade,
  profile_id uuid not null
    references public.profiles (id)
    on delete cascade,
  created_at timestamp with time zone not null default now(),
  constraint game_bookmarks_pkey primary key (game_id, profile_id)
);

create index game_bookmarks_profile_id_idx
on public.game_bookmarks (profile_id);

alter table public.game_bookmarks
enable row level security;

create policy "Service role only"
on public.game_bookmarks
as permissive
for all
to service_role
using (true)
with check (true);
