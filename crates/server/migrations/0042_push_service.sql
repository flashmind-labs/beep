-- Which push service a token belongs to: Firebase for Android, or APNs for
-- an iPhone, whose sandbox and production servers take different tokens.
ALTER TABLE devices ADD COLUMN service TEXT NOT NULL DEFAULT 'fcm';
