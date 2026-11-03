create or replace function public.handle_new_user()
returns trigger
language plpgsql
security definer
as $$
begin
  -- auto adds user to profiles tab
  insert into public.profiles (id, email)
  values (new.id, new.email);
  return new;
end;
$$;

drop trigger if exists on_auth_user_created on auth.users;

create trigger on_auth_user_created
after insert on auth.users
for each row
execute function public.handle_new_user();

create or replace function public.set_gameworld_version()
returns trigger
language plpgsql
as $$
begin
  -- auto increment game version number
  new.version_num := coalesce(
    (select max(gameworld_row.version_num) + 1 from public.gameworlds gameworld_row where gameworld_row.game_id = new.game_id),
    1
  );
  return new;
end;
$$;

drop trigger if exists gameworlds_version_trigger on public.gameworlds;

create trigger gameworlds_version_trigger
before insert on public.gameworlds
for each row
execute function public.set_gameworld_version();
