-- Add migration script here
CREATE TABLE IF NOT EXISTS server (
    server_id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT UNIQUE NOT NULL
);

CREATE TABLE IF NOT EXISTS player (
    player_id INTEGER NOT NULL,
    server_id INTEGER NOT NULL REFERENCES server (server_id),
    name TEXT NOT NULL,
    is_removed BOOL NOT NULL DEFAULT 0,
    PRIMARY KEY (server_id, player_id)
);

CREATE TABLE IF NOT EXISTS guild (
    guild_id INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id INTEGER NOT NULL REFERENCES server (server_id),
    name TEXT NOT NULL,
    is_removed BOOL NOT NULL DEFAULT 0,
    UNIQUE(server_id, name)
);

CREATE TABLE IF NOT EXISTS description (
    description_id INTEGER PRIMARY KEY AUTOINCREMENT,
    description TEXT UNIQUE
);

CREATE TABLE IF NOT EXISTS otherplayer_resp (
    otherplayer_resp_id INTEGER PRIMARY KEY AUTOINCREMENT,
    otherplayer_resp TEXT UNIQUE
);

CREATE TABLE IF NOT EXISTS player_info (
    player_info_id INTEGER PRIMARY KEY AUTOINCREMENT,

    player_id INTEGER NOT NULL,
    server_id INTEGER NOT NULL,
    fetch_time INTEGER NOT NULL,

    xp INTEGER NOT NULL,
    level INTEGER NOT NULL,
    soldier_advice INTEGER NOT NULL,
    description_id INTEGER NOT NULL,
    guild_id INTEGER REFERENCES guild(guild_id),

    otherplayer_resp_id INTEGER NOT NULL REFERENCES otherplayer_resp(otherplayer_resp_id),
    FOREIGN KEY (server_id, player_id) REFERENCES player (server_id, player_id)
);

CREATE INDEX IF NOT EXISTS player_info_idx ON player_info(player_id, server_id, fetch_time);

CREATE TABLE IF NOT EXISTS equipment (
    server_id INTEGER NOT NULL,
    player_id INTEGER NOT NULL,
    ident INT NOT NULL,
    FOREIGN KEY (server_id, player_id) REFERENCES player (server_id, player_id)
);

CREATE INDEX IF NOT EXISTS equipment_lookup_idx ON equipment (server_id, ident, player_id);
CREATE INDEX IF NOT EXISTS equipment_idx ON equipment (server_id, player_id);
