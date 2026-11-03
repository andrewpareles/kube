-- New accounts start with 2500 build credits, $25 of AI credits.
alter table public.user_profiles alter column build_credits set default 2500;
