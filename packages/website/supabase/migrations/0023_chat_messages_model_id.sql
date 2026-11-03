-- chat_messages.model_id records which Bedrock model served the call that produced a row. It is
-- set on the rows that carry billed token counts (assistant rows and compaction rows), so cost
-- stays derivable as tokens x per-model rate even across model switches. Dollar amounts are
-- deliberately never stored - they are recomputed at query time from a rate table.

alter table public.chat_messages
add column model_id text;

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

  insert into public.chat_messages (game_id, seq, kind, message, working_context_after, model_id, billed_input_tokens, billed_output_tokens)
  select
    input_game_id,
    first_seq + input_row.ordinality - 1,
    input_row.value->>'kind',
    nullif(input_row.value->'message', 'null'::jsonb),
    nullif(input_row.value->'working_context_after', 'null'::jsonb),
    input_row.value->>'model_id',
    (input_row.value->>'billed_input_tokens')::integer,
    (input_row.value->>'billed_output_tokens')::integer
  from jsonb_array_elements(input_rows) with ordinality as input_row(value, ordinality);

  return first_seq;
end;
$$;
