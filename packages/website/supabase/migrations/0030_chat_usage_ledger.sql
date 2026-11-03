-- Spend gets its own table, separate from the conversation it happened in, because the two have
-- different lifecycles. chat_messages is display state: it belongs to a game and should die with
-- it. Usage is a financial record - it has to outlive the game and must not move when the game
-- changes hands.
--
-- The two problems this fixes, both of which the join-through-game_metadata.creator_id approach
-- has by construction:
--
--   1. Attribution moved with ownership. creator_id is mutable, so re-assigning a game silently
--      re-attributed every past charge to its new owner. profile_id here is stamped at write time
--      and never derived, so history stays fixed no matter what happens to the game afterwards.
--   2. Deleting a game erased its spend. chat_messages cascades from game_metadata, so a user
--      could wipe their own usage record by deleting games. game_id here is nullable and set null
--      on delete: the game can go, the charge stays.
--
-- Every row is one model call, so unlike chat_messages there is no "not applicable" case for the
-- input counters - they are exact whenever a row exists. Output is different: it is only reported
-- when a call runs to completion, so it stays nullable and carries a flag saying whether the
-- figure was reported or reconstructed. Unknown, estimated and zero are three different states
-- and collapsing them under-bills silently.
--
-- Rows are inserted directly by the API rather than through save_chat_turn, and on a task that
-- outlives the request. A call is billed by Bedrock the moment it completes, so recording it must
-- not depend on the turn finishing, the tools running, or the user still being connected - a user
-- who quits mid-message has still been charged. That also leaves save_chat_turn untouched, so
-- this migration adds only new objects and can be applied at any time.
--
-- The four counters are kept apart because they price and burn quota differently: cache reads are
-- ~a tenth the price of fresh input and cost no TPM quota at all, while output burns 10x on Claude
-- Fable 5.1. Dollar amounts stay uncomputed, as established in 0023 - they are derived at query
-- time from these counters and model_id against a rate table.

create table public.chat_usage (
  id bigint generated always as identity primary key,
  -- The model call this charge is for (a Bedrock invocation - unrelated to a tool call id). The same id lands on every chat_messages row that call
  -- produced, so joining on it returns the messages a charge paid for. It is one-to-many, not
  -- one-to-one: a single call writes the assistant text plus one row per tool call. It can also
  -- match nothing at all - an abandoned turn is billed without its messages ever being saved.
  -- Unique, so a retried insert cannot charge twice. The API retries this write, and a retry
  -- after a *lost response* would otherwise commit a second row for a call that was already
  -- recorded - a silent double-charge. With the constraint the retry conflicts instead, and the
  -- API asks Postgres to ignore the duplicate rather than raise.
  call_id uuid not null unique,
  -- stamped from the authenticated caller, not looked up from the game
  profile_id uuid not null
    references public.user_profiles (id)
    on delete cascade,
  -- which conversation this was spent in, for attribution only; the charge survives the game
  game_id bigint
    references public.game_metadata (game_id)
    on delete set null,
  model_id text not null,
  -- The input side is exact whenever a row exists: the model reports it up front, and a call
  -- that never got that far is not recorded at all.
  input_tokens integer not null default 0,
  cache_write_tokens integer not null default 0,
  cache_read_tokens integer not null default 0,
  -- Nullable on purpose. The model only reports its final output count when a call finishes, so
  -- an abandoned one may have no figure at all. Storing 0 there would assert the call produced
  -- nothing, which is a different claim from not knowing - and one that quietly under-bills.
  output_tokens integer,
  -- Set when output_tokens was reconstructed instead of reported. Such a number is a floor:
  -- thinking is billed as output but arrives with empty text, so it cannot be counted from the
  -- wire. These rows are the ones to reconcile against CloudWatch if exactness matters.
  output_tokens_estimated boolean not null default false,
  -- Bedrock's own request id for this call. Stored because a row whose output is estimated can
  -- only be reconciled against AWS's invocation logs if there is a key both sides share - our
  -- call_id is generated locally and means nothing to AWS.
  aws_request_id text,
  created_at timestamp with time zone not null default now()
);

-- Per-user-per-period rollups are the main query this table exists to answer.
create index chat_usage_profile_created_idx on public.chat_usage (profile_id, created_at);
-- ...and "what did this exchange cost", joining back to the messages.
-- call_id is already indexed by its unique constraint.

-- The other half of that join. Nullable because user rows are not produced by a model call, and
-- because rows written before this column existed have no call to point at.
alter table public.chat_messages add column call_id uuid;

create index chat_messages_call_id_idx on public.chat_messages (call_id);

-- Same three-argument signature, so the currently deployed API keeps calling it unchanged and
-- simply writes a null call_id. Nothing here breaks an older binary.
create or replace function public.save_chat_turn(input_game_id bigint, input_rows jsonb, input_working_context jsonb)
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

  insert into public.chat_messages (game_id, seq, kind, message, working_context_after, model_id, call_id, billed_input_tokens, billed_output_tokens)
  select
    input_game_id,
    first_seq + input_row.ordinality - 1,
    input_row.value->>'kind',
    nullif(input_row.value->'message', 'null'::jsonb),
    nullif(input_row.value->'working_context_after', 'null'::jsonb),
    input_row.value->>'model_id',
    (input_row.value->>'call_id')::uuid,
    (input_row.value->>'billed_input_tokens')::integer,
    (input_row.value->>'billed_output_tokens')::integer
  from jsonb_array_elements(input_rows) with ordinality as input_row(value, ordinality);

  return first_seq;
end;
$$;

alter table public.chat_usage enable row level security;

create policy "Service role only" on public.chat_usage
  as permissive for all to service_role using (true) with check (true);
