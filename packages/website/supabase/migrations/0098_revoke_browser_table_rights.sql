-- The website reads tables as postgres and the servers use the secret key, so the browser keys never touch a table.
-- Taking their rights away keeps a table closed to them even if its row level security is missing or wrong.
revoke all on all tables in schema public from anon, authenticated;
revoke all on all sequences in schema public from anon, authenticated;

alter default privileges for role postgres in schema public revoke all on tables from anon, authenticated;
alter default privileges for role postgres in schema public revoke all on sequences from anon, authenticated;

-- A revoke only warns when it can't take a right away, so fail here rather than leave one behind.
do $$
begin
    if exists (
        select 1 from pg_class c
        cross join lateral aclexplode(c.relacl) acl
        where c.relnamespace = 'public'::regnamespace and acl.grantee in ('anon'::regrole, 'authenticated'::regrole)
    ) then
        raise exception 'A browser key still has rights on a table or sequence in public';
    end if;
end;
$$;
