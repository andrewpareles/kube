insert into storage.buckets (id, name, public)
values ('chat-screenshots', 'chat-screenshots', false)
on conflict (id) do update
set name = excluded.name,
    public = excluded.public;
