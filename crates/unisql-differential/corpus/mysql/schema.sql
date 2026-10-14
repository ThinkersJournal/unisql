-- SPDX-License-Identifier: MIT OR Apache-2.0
-- Tables of the curated corpus (corpus/mysql/curated.sql), then the three tables of the
-- auth-framework MySQL storage backend (verbatim from auth-framework@881c696d, src/storage/mysql.rs).
-- Statements are separated by a line holding only `;`.
CREATE TABLE users (id integer PRIMARY KEY, name varchar(64) NOT NULL, email varchar(64), age integer,
  active tinyint(1) NOT NULL DEFAULT 1, score double, city varchar(64), created_at datetime,
  meta json);
;
CREATE TABLE orders (id integer PRIMARY KEY, user_id integer NOT NULL, amount integer NOT NULL,
  status varchar(16) NOT NULL, created_at datetime);
;
CREATE TABLE kv (k varchar(64) PRIMARY KEY, v varchar(64), n integer);
;
CREATE TABLE events (id integer PRIMARY KEY, at datetime NOT NULL);
;
CREATE TABLE IF NOT EXISTS auth_tokens (
                token_id     VARCHAR(255)  NOT NULL PRIMARY KEY,
                user_id      VARCHAR(255)  NOT NULL,
                access_token LONGTEXT      NOT NULL,
                refresh_token TEXT,
                token_type   VARCHAR(50),
                expires_at   DATETIME(6)   NOT NULL,
                scopes       TEXT,
                issued_at    DATETIME(6)   NOT NULL,
                auth_method  VARCHAR(100)  NOT NULL,
                subject      VARCHAR(255),
                issuer       VARCHAR(255),
                client_id    VARCHAR(255),
                metadata     JSON,
                created_at   DATETIME(6)   DEFAULT CURRENT_TIMESTAMP(6),
                INDEX idx_auth_tokens_user_id (user_id),
                INDEX idx_auth_tokens_expires_at (expires_at),
                INDEX idx_auth_tokens_access_token (access_token(255))
            ) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci
;
CREATE TABLE IF NOT EXISTS sessions (
                session_id    VARCHAR(255) NOT NULL PRIMARY KEY,
                user_id       VARCHAR(255) NOT NULL,
                data          JSON         NOT NULL,
                expires_at    DATETIME(6),
                created_at    DATETIME(6)  DEFAULT CURRENT_TIMESTAMP(6),
                last_activity DATETIME(6)  DEFAULT CURRENT_TIMESTAMP(6),
                ip_address    VARCHAR(45),
                user_agent    TEXT,
                INDEX idx_sessions_user_id (user_id),
                INDEX idx_sessions_expires_at (expires_at)
            ) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci
;
CREATE TABLE IF NOT EXISTS kv_store (
                `key`      VARCHAR(512) NOT NULL PRIMARY KEY,
                value      LONGBLOB     NOT NULL,
                expires_at DATETIME(6),
                created_at DATETIME(6)  DEFAULT CURRENT_TIMESTAMP(6),
                INDEX idx_kv_store_expires_at (expires_at)
            ) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci
