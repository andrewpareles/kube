-- Safe to run before the deploy: no code reads old-format chat rows any more, and the new chat code
-- expects every row to be an AI SDK message. Deletes the old chat's messages and its model calls,
-- which were never charged credits.
delete from public.chat_messages where message_format <> 'ai-sdk';
delete from public.chat_usage where credits_charged is null;
