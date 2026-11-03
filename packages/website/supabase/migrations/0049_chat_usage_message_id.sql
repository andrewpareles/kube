-- Run before deploying the editor server and website that stop writing kind, message_format and
-- chat_usage.call_id. Code already deployed keeps working, since every column it writes still exists.
alter table public.chat_usage add column message_id text;
alter table public.chat_usage alter column call_id drop not null;
alter table public.chat_messages alter column kind drop not null;
alter table public.chat_messages alter column message_format set default 'ai-sdk';
