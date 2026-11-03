-- Remove the chat revert/undo feature. Reverts appended a 'revert' row that rewound the visible
-- conversation on replay; that ability is gone, so drop its function, column, any existing revert
-- rows, and narrow the kind check to the remaining kinds.

drop function if exists public.revert_chat(bigint, bigint);

delete from public.chat_messages where kind = 'revert';

alter table public.chat_messages
drop column if exists revert_to_seq;

alter table public.chat_messages
drop constraint if exists chat_messages_kind_check;

alter table public.chat_messages
add constraint chat_messages_kind_check
check (kind in ('user', 'assistant', 'compaction'));
