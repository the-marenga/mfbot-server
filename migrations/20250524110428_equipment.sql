-- Add migration script here
CREATE TABLE
    IF NOT EXISTS raw_player (
        raw_player_id INTEGER PRIMARY KEY AUTOINCREMENT,
        fetch_date TEXT NOT NULL,
        name TEXT NOT NULL,
        server TEXT NOT NULL,
        info TEXT NOT NULL,
        description TEXT,
        guild TEXT,
        soldier_advice INTEGER
    );

CREATE TABLE
    IF NOT EXISTS server (
        server_id INTEGER PRIMARY KEY AUTOINCREMENT,
        url TEXT NOT NULL
    );

CREATE INDEX IF NOT EXISTS server_url_idx on server (url);

CREATE TABLE
    IF NOT EXISTS player (
        player_id INT NOT NULL,
        server_id INT NOT NULL REFERENCES server (server_id),
        name TEXT NOT NULL,
        last_online INT NOT NULL,
        last_updated INT NOT NULL,
        PRIMARY KEY (server_id, player_id)
    );

CREATE TABLE
    IF NOT EXISTS equipment (
        server_id INT NOT NULL,
        player_id INT NOT NULL,
        slot INT NOT NULL,
        ident INT,
        PRIMARY KEY (server_id, player_id, slot),
        FOREIGN KEY (server_id, player_id) REFERENCES player (server_id, player_id)
    );

CREATE INDEX IF NOT EXISTS equipment_lookup_idx on equipment (server_id, ident, player_id);