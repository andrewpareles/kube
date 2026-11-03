-- Site admins see every game on /admin and open any game's editor the way someone the creator invited would.
-- Admins are added by hand. Browser keys can't read or write the list.
create table public.site_admins (
    profile_id uuid primary key references public.user_profiles(id) on delete cascade
);
alter table public.site_admins enable row level security;
create policy "Service role only" on public.site_admins for all to service_role using (true) with check (true);
revoke all on table public.site_admins from anon, authenticated;

create or replace function public.editor_browser_access(input_game_id bigint, input_request_id text, input_profile_id uuid, input_invite_token text)
returns uuid
language plpgsql
set search_path = ''
as $$
declare
    game_creator_id uuid;
begin
    select g.creator_id into game_creator_id
    from public.game_metadata g
    where g.game_id = input_game_id and (
        g.creator_id = input_profile_id
        or exists (
            select 1 from public.editor_share_auth s
            where s.game_id = g.game_id and s.invite_token = input_invite_token and s.invite_token <> ''
        )
        or exists (select 1 from public.site_admins a where a.profile_id = input_profile_id)
    );
    if not found then
        raise sqlstate 'PT403' using message = 'You do not have access to this editor. The invite link may have been revoked.';
    end if;
    if input_request_id is not null and not exists (
        select 1 from public.running_editor_servers where request_id = input_request_id and game_id = input_game_id
    ) then
        raise sqlstate 'PT410' using message = 'Editor server stopped. Reopen the editor.';
    end if;
    return game_creator_id;
end;
$$;
