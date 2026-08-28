-- Prefixed like every toolbox component's, because diesel keeps all migrations
-- in one table whose name it does not let you change.

-- The transactional outbox. An event is written here in the same transaction
-- as the domain change that produced it, and a relay publishes it afterwards.
-- That is what makes a later broker swap a config change rather than an audit
-- of every publish site: delivery was already at-least-once.
CREATE TABLE toolbox_outbox (
    id BIGSERIAL PRIMARY KEY,
    topic TEXT NOT NULL,
    event JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at TIMESTAMPTZ
);

-- Partial: the relay only ever looks at unpublished rows, and this keeps the
-- index the size of the backlog rather than the size of history.
CREATE INDEX idx_toolbox_outbox_unpublished
    ON toolbox_outbox (id) WHERE published_at IS NULL;

CREATE TABLE toolbox_kv (
    key TEXT PRIMARY KEY,
    value BYTEA NOT NULL,
    expires_at TIMESTAMPTZ
);

CREATE INDEX idx_toolbox_kv_expires ON toolbox_kv (expires_at) WHERE expires_at IS NOT NULL;

-- Leases, not pg_advisory_lock. An advisory lock belongs to a *session*, and
-- a pooled connection is returned between statements - so the next caller can
-- be handed the same session, where advisory locks are re-entrant, and take a
-- lock somebody else is holding. A lease row is owned by whoever wrote it,
-- whichever connection they happen to be using, and it expires on its own if
-- the holder dies or hangs.
CREATE TABLE toolbox_locks (
    key TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);
