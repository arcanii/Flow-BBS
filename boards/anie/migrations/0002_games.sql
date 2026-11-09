-- The game room: each member's best time at Mines for each of the sizes it
-- keeps times for, the leaderboard members share.
CREATE TABLE mines_best (
    member_id   bigint NOT NULL REFERENCES members (id) ON DELETE CASCADE,
    size        text NOT NULL CHECK (size IN ('Small', 'Medium', 'Large')),
    seconds     double precision NOT NULL CHECK (seconds > 0),
    at          timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (member_id, size)
);

CREATE INDEX mines_best_fastest ON mines_best (size, seconds);
