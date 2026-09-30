-- A token is asked about at the push service before its device is
-- registered. When the service said it is one it can push to is written
-- here. Empty: the service could not say at the time, or the device was
-- registered before tokens were asked about; the first push decides.
ALTER TABLE devices ADD COLUMN token_checked_at INTEGER;
