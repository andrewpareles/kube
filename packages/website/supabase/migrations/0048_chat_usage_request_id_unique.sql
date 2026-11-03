-- One row per Bedrock call. aws_request_id is Bedrock's x-amzn-requestid, so a call always carries the
-- same key, while call_id is made fresh by the editor server on every insert and so can never stop a
-- call being recorded twice. With this, the editor server inserts with on conflict do nothing, and a
-- credit charge made in that same statement can only ever happen once per call.
--
-- Concurrently, so the index builds without blocking usage rows being written. That cannot run inside
-- a transaction: apply this file with plain psql -f, not --single-transaction.
create unique index concurrently if not exists chat_usage_aws_request_id_key
  on public.chat_usage (aws_request_id);
