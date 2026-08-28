-- Prefixed, because two toolbox components can share one database and diesel
-- keeps every migration in one table whose name it does not let you change.
-- The version, not the table, is what keeps them from colliding.
CREATE TABLE toolbox_files (
    key TEXT PRIMARY KEY,
    hash TEXT NOT NULL,
    filename TEXT,
    mime_type TEXT NOT NULL,
    size BIGINT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    deleted_at TIMESTAMP
);

CREATE INDEX idx_toolbox_files_hash ON toolbox_files (hash);
