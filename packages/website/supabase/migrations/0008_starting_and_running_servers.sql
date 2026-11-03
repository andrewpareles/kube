create table public.starting_servers (
    request_id text primary key,
    game_id bigint not null,
    gameworld_version_num bigint not null,
    max_players integer not null,
    starting_at timestamptz not null,
    constraint starting_servers_game_id_fkey
        foreign key (game_id)
        references public."games-metadata" (game_id),
    constraint starting_servers_gameworld_fkey
        foreign key (game_id, gameworld_version_num)
        references public.gameworlds (game_id, version_num)
);

alter table public.starting_servers
enable row level security;

create policy "Service role only"
on public.starting_servers
as permissive
for all
to service_role
using (true)
with check (true);

create table public.running_servers (
    request_id text primary key,
    game_id bigint not null,
    gameworld_version_num bigint not null,
    max_players integer not null,
    public_ip text not null,
    external_port integer not null,
    constraint running_servers_game_id_fkey
        foreign key (game_id)
        references public."games-metadata" (game_id),
    constraint running_servers_gameworld_fkey
        foreign key (game_id, gameworld_version_num)
        references public.gameworlds (game_id, version_num)
);

alter table public.running_servers
enable row level security;

create policy "Service role only"
on public.running_servers
as permissive
for all
to service_role
using (true)
with check (true);

insert into public.running_servers (
    request_id,
    game_id,
    gameworld_version_num,
    max_players,
    public_ip,
    external_port
)
select
    deployment_row.request_id,
    deployment_row.game_id,
    deployment_row.gameworld_version_num,
    deployment_row.max_players,
    deployment_row.public_ip,
    deployment_row.external_port
from public.edgegap_deployments as deployment_row
where deployment_row.public_ip is not null
  and deployment_row.external_port is not null
  and lower(deployment_row.status) not like '%error%'
  and lower(deployment_row.status) not like '%failed%'
  and lower(deployment_row.status) not like '%terminated%'
  and lower(deployment_row.status) not like '%stopped%'
  and lower(deployment_row.status) not like '%deleted%'
  and lower(deployment_row.status) not like '%removed%';

insert into public.starting_servers (
    request_id,
    game_id,
    gameworld_version_num,
    max_players,
    starting_at
)
select
    deployment_row.request_id,
    deployment_row.game_id,
    deployment_row.gameworld_version_num,
    deployment_row.max_players,
    now()
from public.edgegap_deployments as deployment_row
where (deployment_row.public_ip is null or deployment_row.external_port is null)
  and lower(deployment_row.status) not like '%error%'
  and lower(deployment_row.status) not like '%failed%'
  and lower(deployment_row.status) not like '%terminated%'
  and lower(deployment_row.status) not like '%stopped%'
  and lower(deployment_row.status) not like '%deleted%'
  and lower(deployment_row.status) not like '%removed%';

create table public.starting_player_sessions (
    profile_id uuid primary key,
    request_id text not null,
    constraint starting_player_sessions_profile_id_fkey
        foreign key (profile_id)
        references public.profiles (id),
    constraint starting_player_sessions_request_id_fkey
        foreign key (request_id)
        references public.starting_servers (request_id)
        on delete cascade
);

alter table public.starting_player_sessions
enable row level security;

create policy "Users can read own sessions"
on public.starting_player_sessions
as permissive
for select
to public
using (auth.uid() = profile_id);

create policy "Service role only"
on public.starting_player_sessions
as permissive
for all
to service_role
using (true)
with check (true);

create table public.running_player_sessions (
    profile_id uuid primary key,
    request_id text not null,
    constraint running_player_sessions_profile_id_fkey
        foreign key (profile_id)
        references public.profiles (id),
    constraint running_player_sessions_request_id_fkey
        foreign key (request_id)
        references public.running_servers (request_id)
        on delete cascade
);

alter table public.running_player_sessions
enable row level security;

create policy "Users can read own sessions"
on public.running_player_sessions
as permissive
for select
to public
using (auth.uid() = profile_id);

create policy "Service role only"
on public.running_player_sessions
as permissive
for all
to service_role
using (true)
with check (true);

insert into public.running_player_sessions (
    profile_id,
    request_id
)
select
    player_session.profile_id,
    player_session.request_id
from public.player_sessions as player_session
where exists (
    select 1
    from public.running_servers as running_server
    where running_server.request_id = player_session.request_id
);

insert into public.starting_player_sessions (
    profile_id,
    request_id
)
select
    player_session.profile_id,
    player_session.request_id
from public.player_sessions as player_session
where exists (
    select 1
    from public.starting_servers as starting_server
    where starting_server.request_id = player_session.request_id
);

drop table public.player_sessions;

drop table public.edgegap_deployments;
