CREATE TABLE todos (
{% if database == "postgres" %}    id SERIAL PRIMARY KEY,
{% elsif database == "mariadb" %}    id INTEGER PRIMARY KEY AUTO_INCREMENT,
{% else %}    id INTEGER PRIMARY KEY AUTOINCREMENT,
{% endif %}
    title TEXT NOT NULL,
    done BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    deleted_at TIMESTAMP,
    version INTEGER NOT NULL DEFAULT 0
);
