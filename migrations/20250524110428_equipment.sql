CREATE TABLE server (
    server_id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    url TEXT UNIQUE NOT NULL,
    last_hof_crawl INT NOT NULL DEFAULT 0
);

CREATE TABLE todo_hof_page (
    server_id INTEGER NOT NULL REFERENCES server (server_id),
    idx INTEGER NOT NULL,
    next_report_attempt INT NOT NULL DEFAULT 0,
    PRIMARY KEY (server_id, idx)
);

CREATE INDEX hof_todo_idx ON todo_hof_page (server_id);

CREATE TABLE player (
    player_id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    server_id INTEGER NOT NULL REFERENCES server (server_id),
    name TEXT NOT NULL,
    -- The current level of this player
    level INT,
    -- The current xp of this player
    xp INT,
    -- The total sum of all attributes this player has
    attributes INT,
    -- The next time, that this player is scheduled to be looked at again
    next_report_attempt INT NOT NULL,
    -- The last time, that this player was reported at
    last_reported INT,
    -- The last time, that this player has changed in any way (xp/attributes)
    last_changed INT,
    -- The last time this player was confirmed logged in through the guild
    last_online INT,
    -- The amount of equipped items
    equip_count INT,
    -- Wether or not this player has been removed from the server
    is_removed BOOL NOT NULL DEFAULT 0,
    UNIQUE (server_id, name)
);

CREATE INDEX player_stats_idx ON player (player_id, level, attributes)
WHERE
    is_removed = false;

CREATE INDEX player_nude_idx ON player (server_id, equip_count, level, attributes)
WHERE
    is_removed = false;

CREATE INDEX player_crawl_idx ON player (server_id, next_report_attempt)
WHERE
    is_removed = false;

CREATE TABLE guild (
    guild_id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    server_id INTEGER NOT NULL REFERENCES server (server_id),
    name TEXT NOT NULL,
    is_removed BOOL NOT NULL DEFAULT 0,
    UNIQUE (server_id, name)
);

CREATE TABLE description (
    description_id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    description TEXT UNIQUE
);

CREATE TABLE otherplayer_resp (
    otherplayer_resp_id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    otherplayer_resp TEXT UNIQUE
);

CREATE TABLE player_info (
    player_info_id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    player_id INTEGER NOT NULL REFERENCES player (player_id),
    fetch_time INTEGER NOT NULL,
    xp INTEGER NOT NULL,
    level INTEGER NOT NULL,
    soldier_advice INTEGER NOT NULL,
    description_id INTEGER NOT NULL,
    guild_id INTEGER REFERENCES guild (guild_id),
    otherplayer_resp_id INTEGER NOT NULL REFERENCES otherplayer_resp (otherplayer_resp_id)
);

CREATE INDEX player_info_idx ON player_info (player_id, fetch_time);

CREATE TABLE equipment (
    server_id INTEGER NOT NULL,
    player_id INTEGER NOT NULL,
    ident INT NOT NULL,
    FOREIGN KEY (player_id) REFERENCES player (player_id)
);

CREATE INDEX equipment_lookup_idx ON equipment (server_id, player_id, ident);

CREATE INDEX equipment_player_idx ON equipment (player_id);
