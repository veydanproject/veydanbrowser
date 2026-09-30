-- The id of a group is no secret: it is on every event of the group, in the
-- open, and anybody can sign an event that names it. So a group event is
-- pushed only to the devices that registered a push key whose mark is on
-- the event. A push key is made from the key of the group and opens
-- nothing; one or two are kept for a group of a device, the second being
-- the key before the last change.
--
-- The rows of `device_groups` that were there before have no keys here.
-- Such a device is pushed to about no group until it registers again.
--
-- The keys of a group of a device go with that group, as the group goes
-- with its device.
CREATE TABLE device_group_keys (
    device    INTEGER NOT NULL,
    group_id  TEXT NOT NULL,
    -- 64 hex characters.
    push_key  TEXT NOT NULL,
    PRIMARY KEY (device, group_id, push_key),
    FOREIGN KEY (device, group_id)
        REFERENCES device_groups (device, group_id) ON DELETE CASCADE
) WITHOUT ROWID;
-- An event of a group is held against the keys of that group.
CREATE INDEX device_group_keys_group ON device_group_keys (group_id, push_key);

-- `seen_events.flag` has two values more than 002 names: 4 dated too far
-- from now to be pushed, 5 of a group and without a mark of a key
-- registered for it.
