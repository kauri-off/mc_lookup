-- Your SQL goes here

ALTER TABLE servers ADD COLUMN last_seen TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL;