-- Devices and what is watched for them.
--
-- Ids of keys, groups and events are hex text: a person with sqlite3 in a
-- terminal reads and compares them as they are.
--
-- Migrations only add. The release before this one must be able to run on
-- the database after a rollback.

CREATE TABLE devices (
    id           INTEGER PRIMARY KEY,
    -- Owner of the device: the key that signed the registration.
    pubkey       TEXT NOT NULL,
    -- Chosen by the device; one per identity on a phone.
    device_id    TEXT NOT NULL,
    app_id       TEXT NOT NULL,
    provider     TEXT NOT NULL CHECK (provider IN ('fcm', 'apns', 'unifiedpush')),
    -- Address of the device at the push service.
    token        TEXT NOT NULL,
    -- More of the address, for providers that need more (JSON).
    channel_json TEXT,
    locale       TEXT NOT NULL DEFAULT 'en',
    app_version  TEXT,
    pref_dm      INTEGER NOT NULL DEFAULT 1,
    pref_groups  INTEGER NOT NULL DEFAULT 1,
    -- Key of the marks on the owner's own group messages.
    author_key   TEXT,
    state        TEXT NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'dead_token')),
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    last_push_at INTEGER,
    last_outcome TEXT,
    UNIQUE (pubkey, device_id)
);

-- One address at a push service is one device. When a phone registers under
-- another identity, the row of the former one goes.
CREATE UNIQUE INDEX devices_token ON devices (app_id, provider, token);
CREATE INDEX devices_expiry ON devices (expires_at);

CREATE TABLE device_relays (
    device    INTEGER NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    -- Normalized: see relays::normalize.
    url       TEXT NOT NULL,
    dm        INTEGER NOT NULL,
    groups    INTEGER NOT NULL,
    PRIMARY KEY (device, url)
) WITHOUT ROWID;
CREATE INDEX device_relays_url ON device_relays (url);

CREATE TABLE device_groups (
    device    INTEGER NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    group_id  TEXT NOT NULL,
    -- What this device's owner calls the group. Not shared between devices.
    name      TEXT,
    PRIMARY KEY (device, group_id)
) WITHOUT ROWID;
CREATE INDEX device_groups_id ON device_groups (group_id);
