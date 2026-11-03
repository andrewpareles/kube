insert into storage.buckets (id, name, public)
values ('thumbnails', 'thumbnails', true)
on conflict (id) do update
set name = excluded.name,
    public = excluded.public;
