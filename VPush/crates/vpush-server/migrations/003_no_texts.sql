-- The phone reads the message itself now, and the server writes no texts:
-- it needs neither the language of a device nor what the device calls a
-- group.
--
-- This one takes away, which the rule of 001 forbids. It is allowed here
-- because no release before 0.2.0 was in production: there is no database
-- to go back to. A 0.1 release cannot run on a database after this.
ALTER TABLE devices DROP COLUMN locale;
ALTER TABLE device_groups DROP COLUMN name;
