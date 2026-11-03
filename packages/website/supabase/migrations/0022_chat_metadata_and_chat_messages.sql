-- Persistent per-game AI editor chats (one chat per game).
--
-- chat_metadata holds the model-facing working context: the Bedrock wire-format message array
-- that the api server sends to the model. It is a lossy cache of the conversation - appended to
-- every turn and replaced wholesale by compaction and revert - never the source of truth for
-- what the user sees.
--
-- chat_messages is the append-only human-facing log. Rows are never updated or deleted:
-- - kind 'user'/'assistant' rows hold a display ChatMessage in `message`.
-- - kind 'compaction' rows record a summarization call (`message` is the summary text). They are
--   hidden from clients; they exist for auditing and for their token counts.
-- - kind 'revert' rows rewind the visible conversation: on read, replay rows in seq order and
--   truncate the visible list to seq <= revert_to_seq whenever a revert row is hit.
-- - billed_input_tokens/billed_output_tokens are the empirical Bedrock usage of the call that
--   produced the row, so per-user and per-date token accounting is a sum over this table
--   (joined through game_metadata.creator_id).
-- - working_context_after snapshots the working context on the last row of each turn so revert
--   can restore the model's exact context.
-- - world_version is an unused-for-now hook to link a turn to a game_worlds checkpoint.

create table public.chat_metadata (
  game_id bigint not null primary key
    references public.game_metadata (game_id)
    on delete cascade,
  working_context jsonb not null default '[]'::jsonb,
  created_at timestamp with time zone not null default now(),
  updated_at timestamp with time zone not null default now()
);

alter table public.chat_metadata
enable row level security;

create policy "Service role only"
on public.chat_metadata
as permissive
for all
to service_role
using (true)
with check (true);

create table public.chat_messages (
  game_id bigint not null
    references public.game_metadata (game_id)
    on delete cascade,
  seq bigint not null,
  kind text not null
    check (kind in ('user', 'assistant', 'compaction', 'revert')),
  message jsonb,
  revert_to_seq bigint,
  working_context_after jsonb,
  world_version bigint,
  billed_input_tokens integer,
  billed_output_tokens integer,
  created_at timestamp with time zone not null default now(),
  constraint chat_messages_pkey primary key (game_id, seq)
);

alter table public.chat_messages
enable row level security;

create policy "Service role only"
on public.chat_messages
as permissive
for all
to service_role
using (true)
with check (true);

-- Appends one turn's rows and replaces the working context in a single transaction. input_rows
-- is a jsonb array of {kind, message, working_context_after, billed_input_tokens,
-- billed_output_tokens} in chronological order. Returns the seq assigned to the first row.
-- The chat_metadata upsert takes the row lock first, which serializes seq allocation per game.
create function public.save_chat_turn(input_game_id bigint, input_rows jsonb, input_working_context jsonb)
returns bigint
language plpgsql
as $$
declare
  first_seq bigint;
begin
  insert into public.chat_metadata (game_id, working_context)
  values (input_game_id, input_working_context)
  on conflict (game_id) do update
  set working_context = excluded.working_context, updated_at = now();

  select coalesce(max(chat_message_row.seq), 0) + 1 into first_seq
  from public.chat_messages chat_message_row
  where chat_message_row.game_id = input_game_id;

  insert into public.chat_messages (game_id, seq, kind, message, working_context_after, billed_input_tokens, billed_output_tokens)
  select
    input_game_id,
    first_seq + input_row.ordinality - 1,
    input_row.value->>'kind',
    nullif(input_row.value->'message', 'null'::jsonb),
    nullif(input_row.value->'working_context_after', 'null'::jsonb),
    (input_row.value->>'billed_input_tokens')::integer,
    (input_row.value->>'billed_output_tokens')::integer
  from jsonb_array_elements(input_rows) with ordinality as input_row(value, ordinality);

  return first_seq;
end;
$$;

-- Appends a revert row and restores the working context to the latest snapshot at or before
-- input_revert_to_seq ('[]' when reverting past the first snapshot). Rows after the target are
-- kept as a dead branch; the visible conversation is rebuilt on read via the replay rule above.
create function public.revert_chat(input_game_id bigint, input_revert_to_seq bigint)
returns void
language plpgsql
as $$
begin
  update public.chat_metadata
  set working_context = coalesce(
    (select snapshot_row.working_context_after
     from public.chat_messages snapshot_row
     where snapshot_row.game_id = input_game_id
       and snapshot_row.seq <= input_revert_to_seq
       and snapshot_row.working_context_after is not null
     order by snapshot_row.seq desc
     limit 1),
    '[]'::jsonb
  ),
  updated_at = now()
  where game_id = input_game_id;
  if not found then
    -- no chat for this game yet, nothing to revert
    return;
  end if;

  insert into public.chat_messages (game_id, seq, kind, revert_to_seq)
  select input_game_id, coalesce(max(chat_message_row.seq), 0) + 1, 'revert', input_revert_to_seq
  from public.chat_messages chat_message_row
  where chat_message_row.game_id = input_game_id;
end;
$$;
