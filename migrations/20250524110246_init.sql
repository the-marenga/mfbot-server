-- Add migration script here
CREATE TABLE error (
    error_id SERIAL PRIMARY KEY,
    stacktrace TEXT,
    version INT,
    additional_info TEXT,
    os TEXT,
    arch TEXT,
    error_text TEXT,
    hwid TEXT,
    timestamp TIMESTAMP
);
