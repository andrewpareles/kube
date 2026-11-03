-- Run once every server and the website use the asset names from 0054.
drop view public.game_object_catalog;
drop function public.search_game_object_catalog(text, public.asset_type, integer, integer);
