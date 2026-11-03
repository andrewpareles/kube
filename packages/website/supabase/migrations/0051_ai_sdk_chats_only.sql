-- Run after deploying the editor server and website that stop writing kind, message_format,
-- chat_metadata and chat_usage.call_id. Removes the table, function and columns only the old chat used.
-- The deletes repeat 0050 for rows written by the previous code between the two migrations.
delete from public.chat_messages where message_format <> 'ai-sdk';
delete from public.chat_usage where credits_charged is null;

drop function public.save_chat_turn(bigint, jsonb, jsonb);
drop table public.chat_metadata;

alter table public.chat_messages
  drop column kind,
  drop column message_format,
  drop column working_context_after,
  drop column model_id,
  drop column call_id,
  drop column billed_input_tokens,
  drop column billed_output_tokens,
  alter column message set not null;

alter table public.chat_usage
  drop column call_id,
  drop column output_tokens_estimated,
  alter column aws_request_id set not null,
  alter column credits_charged set not null;
