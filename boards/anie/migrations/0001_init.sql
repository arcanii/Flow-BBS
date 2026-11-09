-- The Anie Solutions BBS: its members, their codes, its forums and its bug
-- tracker, and what the sysop did. Applied once, by `anie-bbs` at its
-- start, in a transaction (db.rs).

-- Members. A member applies (unverified until their email's code comes
-- back and their authenticator is set up), waits for the sysop (pending),
-- and is let in (active), turned down (rejected) or shut out (suspended).
-- Roles go member < staff < sysop: staff moderate the forums and triage
-- bugs; the sysop runs the board.
CREATE TABLE members (
    id              bigserial PRIMARY KEY,
    handle          text NOT NULL,
    handle_key      text NOT NULL UNIQUE,          -- lower(handle)
    real_name       text NOT NULL,
    email           text NOT NULL,
    email_key       text NOT NULL UNIQUE,          -- lower(email)
    email_verified  boolean NOT NULL DEFAULT false,
    password_hash   text NOT NULL,                 -- Argon2id
    seed_sealed     bytea,                         -- the authenticator's seed, sealed (crypto.rs)
    last_step       bigint,                        -- the last code's step taken, never taken again
    role            text NOT NULL DEFAULT 'member' CHECK (role IN ('member', 'staff', 'sysop')),
    status          text NOT NULL DEFAULT 'unverified' CHECK (status IN ('unverified', 'pending', 'active', 'rejected', 'suspended')),
    status_note     text NOT NULL DEFAULT '',      -- why rejected or suspended, as the sysop wrote it
    applied_from    text NOT NULL DEFAULT '',
    created_at      timestamptz NOT NULL DEFAULT now(),
    decided_at      timestamptz,
    decided_by      bigint REFERENCES members (id),
    last_login_at   timestamptz,
    failed_logins   integer NOT NULL DEFAULT 0,
    locked_until    timestamptz
);
CREATE INDEX members_status ON members (status, created_at);

-- Recovery codes, a member's way in once each when their phone is lost:
-- keyed hashes only.
CREATE TABLE recovery_codes (
    member_id  bigint NOT NULL REFERENCES members (id) ON DELETE CASCADE,
    code_hash  text NOT NULL,
    used_at    timestamptz,
    PRIMARY KEY (member_id, code_hash)
);

-- Codes mailed to a member: to verify an address (verify), to set a new
-- password (reset), or to prove the address before a new authenticator is
-- set up (enrol). Keyed hashes, used once, 10 minutes, 5 tries.
CREATE TABLE email_codes (
    id          bigserial PRIMARY KEY,
    member_id   bigint REFERENCES members (id) ON DELETE CASCADE,
    email_key   text NOT NULL,
    purpose     text NOT NULL CHECK (purpose IN ('verify', 'reset', 'enrol', 'change')),
    code_hash   text NOT NULL,
    attempts    integer NOT NULL DEFAULT 0,
    consumed    boolean NOT NULL DEFAULT false,
    expires_at  timestamptz NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX email_codes_sent ON email_codes (email_key, created_at DESC);

-- Forums, each read and posted in from a role up.
CREATE TABLE forums (
    id           bigserial PRIMARY KEY,
    name         text NOT NULL UNIQUE,
    description  text NOT NULL DEFAULT '',
    read_role    text NOT NULL DEFAULT 'member' CHECK (read_role IN ('member', 'staff', 'sysop')),
    post_role    text NOT NULL DEFAULT 'member' CHECK (post_role IN ('member', 'staff', 'sysop')),
    position     integer NOT NULL DEFAULT 0,
    archived     boolean NOT NULL DEFAULT false,
    created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE threads (
    id            bigserial PRIMARY KEY,
    forum_id      bigint NOT NULL REFERENCES forums (id),
    title         text NOT NULL,
    author_id     bigint NOT NULL REFERENCES members (id),
    pinned        boolean NOT NULL DEFAULT false,
    locked        boolean NOT NULL DEFAULT false,
    deleted       boolean NOT NULL DEFAULT false,
    posts         integer NOT NULL DEFAULT 0,
    last_post_id  bigint NOT NULL DEFAULT 0,
    last_post_at  timestamptz NOT NULL DEFAULT now(),
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX threads_in_forum ON threads (forum_id, pinned DESC, last_post_at DESC);

CREATE TABLE posts (
    id          bigserial PRIMARY KEY,
    thread_id   bigint NOT NULL REFERENCES threads (id),
    author_id   bigint NOT NULL REFERENCES members (id),
    body        text NOT NULL,
    deleted     boolean NOT NULL DEFAULT false,
    deleted_by  bigint REFERENCES members (id),
    edited_at   timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX posts_in_thread ON posts (thread_id, id);

-- How far each member has read each thread, for what's new.
CREATE TABLE thread_reads (
    member_id     bigint NOT NULL REFERENCES members (id) ON DELETE CASCADE,
    thread_id     bigint NOT NULL REFERENCES threads (id),
    last_post_id  bigint NOT NULL,
    PRIMARY KEY (member_id, thread_id)
);

-- The bug tracker: products, their bugs, and what's said and changed on
-- each.
CREATE TABLE products (
    id           bigserial PRIMARY KEY,
    name         text NOT NULL UNIQUE,
    description  text NOT NULL DEFAULT '',
    archived     boolean NOT NULL DEFAULT false
);

CREATE TABLE bugs (
    id           bigserial PRIMARY KEY,
    product_id   bigint NOT NULL REFERENCES products (id),
    title        text NOT NULL,
    description  text NOT NULL,
    severity     text NOT NULL DEFAULT 'normal' CHECK (severity IN ('low', 'normal', 'high', 'critical')),
    status       text NOT NULL DEFAULT 'new' CHECK (status IN ('new', 'confirmed', 'in-progress', 'resolved', 'closed', 'wontfix')),
    reporter_id  bigint NOT NULL REFERENCES members (id),
    assignee_id  bigint REFERENCES members (id),
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX bugs_open ON bugs (status, updated_at DESC);

CREATE TABLE bug_comments (
    id          bigserial PRIMARY KEY,
    bug_id      bigint NOT NULL REFERENCES bugs (id),
    author_id   bigint NOT NULL REFERENCES members (id),
    body        text NOT NULL DEFAULT '',
    change      text NOT NULL DEFAULT '',          -- "status: new → confirmed", where one was made
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX bug_comments_on ON bug_comments (bug_id, id);

-- The board's own words the sysop can change from the board: the bulletin
-- shown after login.
CREATE TABLE settings (
    key    text PRIMARY KEY,
    value  text NOT NULL
);

-- What happened, for the sysop: logins, applications, decisions, changes.
CREATE TABLE audit (
    id        bigserial PRIMARY KEY,
    at        timestamptz NOT NULL DEFAULT now(),
    actor_id  bigint REFERENCES members (id) ON DELETE SET NULL,
    action    text NOT NULL,
    subject   text NOT NULL DEFAULT '',
    address   text NOT NULL DEFAULT ''
);
CREATE INDEX audit_at ON audit (at DESC);

INSERT INTO forums (name, description, read_role, post_role, position) VALUES
    ('Announcements', 'News from Anie Solutions.', 'member', 'staff', 0),
    ('General', 'Anything and everything.', 'member', 'member', 1),
    ('Support', 'Questions about our products, answered.', 'member', 'member', 2),
    ('Staff room', 'For Anie staff only.', 'staff', 'staff', 3);

INSERT INTO products (name, description) VALUES
    ('Anie Solutions BBS', 'This board.'),
    ('General', 'Anything without a product of its own.');

INSERT INTO settings (key, value) VALUES
    ('bulletin', 'Welcome to the Anie Solutions BBS. Say hello in General, ask in Support, and file what''s broken in the bug tracker.');
