-- What the server has seen on the relays, and what it has taken stock of.

-- Every event the server has dealt with, so that it deals with it once:
-- the same event comes from several relays, and comes again after every
-- reconnect. Kept for three days; relays are asked for two.
CREATE TABLE seen_events (
    event_id TEXT PRIMARY KEY,
    -- 0 pushed, 1 there before the watch began, 2 marked as not worth a
    -- push, 3 for nobody who is registered
    flag     INTEGER NOT NULL,
    seen_at  INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX seen_events_at ON seen_events (seen_at);

-- A key or a group newly watched on a relay has events there from before.
-- They are taken stock of, not pushed; a row here says that was done.
CREATE TABLE baselines (
    url    TEXT NOT NULL,
    kind   TEXT NOT NULL CHECK (kind IN ('dm', 'group')),
    target TEXT NOT NULL,
    at     INTEGER NOT NULL,
    PRIMARY KEY (url, kind, target)
) WITHOUT ROWID;

-- When the server last had a relay on the line: where to ask from after
-- it was away.
CREATE TABLE relay_state (
    url           TEXT PRIMARY KEY,
    last_alive_at INTEGER NOT NULL
) WITHOUT ROWID;
