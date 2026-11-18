-- A browser joins a game server over HTTP/3 on the second UDP port Edgegap publishes for it, so the row that holds a
-- running server's QUIC address holds its browser port too. A deployment that never published one leaves it null, and
-- the page is then told the server is not serving browsers rather than being sent to a port that answers nothing.
alter table public.running_servers add column webtransport_port integer;
