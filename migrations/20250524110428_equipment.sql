CREATE TABLE server (
    server_id SERIAL PRIMARY KEY,
    url TEXT UNIQUE NOT NULL,
    last_hof_crawl TIMESTAMP NOT NULL DEFAULT now ()
);

CREATE TABLE todo_hof_page (
    server_id INT NOT NULL REFERENCES server (server_id),
    idx INT NOT NULL,
    next_report_attempt TIMESTAMP NOT NULL DEFAULT now (),
    PRIMARY KEY (server_id, idx)
);

CREATE INDEX hof_todo_idx ON todo_hof_page (server_id);

CREATE TABLE player (
    player_id SERIAL PRIMARY KEY,
    server_id INT NOT NULL REFERENCES server (server_id),
    name TEXT NOT NULL,
    -- The current level of this player
    level INT,
    -- The current xp of this player
    xp BIGINT,
    -- The current xp of this player
    honor INT,
    -- The total sum of all attributes this player has
    attributes BIGINT,
    -- The next time, that this player is scheduled to be looked at again
    next_report_attempt TIMESTAMP NOT NULL DEFAULT now (),
    -- The last time, that this player was reported at
    last_reported TIMESTAMP,
    -- The last time, that this player has changed in any way (xp/attributes)
    last_changed TIMESTAMP,
    -- The last time this player was confirmed logged in through the guild
    last_online TIMESTAMP,
    -- The amount of equipped items
    equip_count SMALLINT,
    -- Wether or not this player has been removed from the server
    is_removed BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (server_id, name)
);

CREATE INDEX player_stats_idx ON player (level, attributes) INCLUDE (player_id, name)
WHERE
    is_removed = false;

CREATE INDEX player_nude_idx ON player (server_id, attributes, level)
WHERE
    is_removed = false
    AND equip_count < 3;

CREATE INDEX player_crawl_idx ON player (server_id, next_report_attempt)
WHERE
    is_removed = false;

CREATE TABLE guild (
    guild_id SERIAL PRIMARY KEY,
    server_id INT NOT NULL REFERENCES server (server_id),
    name TEXT NOT NULL,
    is_removed BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (server_id, name)
);

CREATE TABLE description (
    description_id SERIAL PRIMARY KEY,
    description TEXT UNIQUE
);

CREATE TABLE otherplayer_resp (
    otherplayer_resp_id SERIAL PRIMARY KEY,
    otherplayer_resp BYTEA NOT NULL,
    hash TEXT NOT NULL UNIQUE
);

CREATE TABLE player_info (
    player_info_id SERIAL PRIMARY KEY,
    player_id INT NOT NULL REFERENCES player (player_id),
    fetch_time TIMESTAMP NOT NULL,
    xp BIGINT NOT NULL,
    honor INT NOT NULL,
    level INT NOT NULL,
    soldier_advice BIGINT NOT NULL,
    description_id INT NOT NULL,
    guild_id INT REFERENCES guild (guild_id),
    otherplayer_resp_id INT NOT NULL REFERENCES otherplayer_resp (otherplayer_resp_id)
);

CREATE INDEX player_info_idx ON player_info (player_id, fetch_time);

CREATE TABLE equipment (
    server_id INT NOT NULL,
    player_id INT NOT NULL REFERENCES player (player_id),
    ident INT NOT NULL
);

CREATE INDEX equipment_lookup_idx ON equipment (server_id, player_id, ident);

CREATE INDEX equipment_player_idx ON equipment (player_id);
