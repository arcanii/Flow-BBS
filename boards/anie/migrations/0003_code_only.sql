-- Logging in by the authenticator's code alone, the password left out: a
-- member's own choice (code_only). And a cap on a handle's failed logins in
-- a day, counted from the first of them (day_failures, day_failures_since):
-- past it the handle is held, and logging in takes a code mailed to the
-- member as well, until they've proved their address.
ALTER TABLE members
    ADD COLUMN code_only boolean NOT NULL DEFAULT false,
    ADD COLUMN day_failures integer NOT NULL DEFAULT 0,
    ADD COLUMN day_failures_since timestamptz,
    ADD COLUMN held boolean NOT NULL DEFAULT false;

-- A mailed code for logging in while a handle is held (login).
ALTER TABLE email_codes DROP CONSTRAINT email_codes_purpose_check;
ALTER TABLE email_codes ADD CONSTRAINT email_codes_purpose_check CHECK (purpose IN ('verify', 'reset', 'enrol', 'change', 'login'));
