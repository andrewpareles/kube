create table public.user_favorites (
  game_id bigint not null
    references public.game_metadata (game_id)
    on delete cascade,
  profile_id uuid not null
    references public.user_profiles (id)
    on delete cascade,
  created_at timestamp with time zone not null default now(),
  constraint user_favorites_pkey primary key (game_id, profile_id)
);

create index user_favorites_profile_id_idx
on public.user_favorites (profile_id);

alter table public.user_favorites
enable row level security;

create policy "Service role only"
on public.user_favorites
as permissive
for all
to service_role
using (true)
with check (true);
