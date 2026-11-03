-- The games we pick out by hand and show as Our Picks. Nothing on the site writes to this table:
-- rows go in and out through Supabase by hand, which is why there is no policy for anyone but the
-- service role.
--
-- our_picks_order is what decides the order they are shown in, smallest first. It is a real number
-- rather than a whole one so a game can be dropped between two others - 1 and 2 leave room for
-- 1.5 - without renumbering everything after it.
create table public.our_picks_games (
  game_id bigint not null
    references public.game_metadata (game_id)
    on delete cascade,
  our_picks_order double precision not null,
  created_at timestamp with time zone not null default now(),
  constraint our_picks_games_pkey primary key (game_id)
);

create index our_picks_games_order_idx
on public.our_picks_games (our_picks_order);

alter table public.our_picks_games
enable row level security;

create policy "Service role only"
on public.our_picks_games
as permissive
for all
to service_role
using (true)
with check (true);
