-- Add migration script here
CREATE TABLE IF NOT EXISTS error (
    error_id INTEGER PRIMARY KEY AUTOINCREMENT,
    stacktrace TEXT,
    version INT,
    additional_info TEXT,
    os TEXT,
    arch TEXT,
    error_text TEXT,
    hwid TEXT,
    timestamp TEXT
)