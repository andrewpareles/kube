create table public.player_auth (
    profile_id uuid not null,
    play_auth_hash text,
    edit_auth_hash text,
    constraint player_auth_pkey primary key (profile_id),
    constraint player_auth_profile_id_fkey
        foreign key (profile_id)
        references public.profiles(id)
);

alter table public.player_auth
enable row level security;

create policy "Service role only"
on public.player_auth
as permissive
for all
to service_role
using (true)
with check (true);

insert into public.player_auth (
    profile_id,
    play_auth_hash,
    edit_auth_hash
)
select
    player_session.profile_id,
    player_session.play_auth_hash,
    player_session.edit_auth_hash
from public.player_sessions as player_session
where player_session.play_auth_hash is not null
   or player_session.edit_auth_hash is not null;

delete from public.player_sessions as player_session
where player_session.request_id is null
   or player_session.left_at is not null
   or not exists (
        select 1
        from public.edgegap_deployments as deployment_row
        where deployment_row.request_id = player_session.request_id
    );

alter table public.player_sessions
drop constraint player_sessions_request_id_fkey;

alter table public.player_sessions
drop column play_auth_hash;

alter table public.player_sessions
drop column edit_auth_hash;

alter table public.player_sessions
drop column left_at;

alter table public.player_sessions
alter column request_id set not null;

alter table public.player_sessions
add constraint player_sessions_request_id_fkey
foreign key (request_id)
references public.edgegap_deployments(request_id)
on delete cascade;
