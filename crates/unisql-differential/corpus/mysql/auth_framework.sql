-- SPDX-License-Identifier: MIT OR Apache-2.0
-- DML statements of the auth-framework MySQL storage backend, read at auth-framework@881c696d
-- (src/storage/mysql.rs, extracted by measure/extract.py; whitespace collapsed). A real
-- project's statements: the origin of each is file:line.

-- auth_143 auth_insert auth-framework@881c696d:src/storage/mysql.rs:143
INSERT INTO auth_tokens ( token_id, user_id, access_token, refresh_token, token_type, expires_at, scopes, issued_at, auth_method, subject, issuer, client_id, metadata ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON DUPLICATE KEY UPDATE access_token = VALUES(access_token), refresh_token = VALUES(refresh_token), expires_at = VALUES(expires_at)

-- auth_181 auth_select auth-framework@881c696d:src/storage/mysql.rs:181
SELECT token_id, user_id, access_token, refresh_token, token_type, expires_at, scopes, issued_at, auth_method, subject, issuer, client_id, metadata FROM auth_tokens WHERE token_id = ?

-- auth_271 auth_select auth-framework@881c696d:src/storage/mysql.rs:271
SELECT token_id, user_id, access_token, refresh_token, token_type, expires_at, scopes, issued_at, auth_method, subject, issuer, client_id, metadata FROM auth_tokens WHERE access_token = ?

-- auth_360 auth_update auth-framework@881c696d:src/storage/mysql.rs:360
UPDATE auth_tokens SET access_token = ?, refresh_token = ?, token_type = ?, expires_at = ?, scopes = ?, issued_at = ?, auth_method = ?, subject = ?, issuer = ?, client_id = ?, metadata = ? WHERE token_id = ?

-- auth_398 auth_delete auth-framework@881c696d:src/storage/mysql.rs:398
DELETE FROM auth_tokens WHERE token_id = ?

-- auth_412 auth_select auth-framework@881c696d:src/storage/mysql.rs:412
SELECT token_id, user_id, access_token, refresh_token, token_type, expires_at, scopes, issued_at, auth_method, subject, issuer, client_id, metadata FROM auth_tokens WHERE user_id = ?

-- auth_505 auth_insert auth-framework@881c696d:src/storage/mysql.rs:505
INSERT INTO sessions (session_id, user_id, created_at, expires_at, last_activity, ip_address, user_agent, data) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON DUPLICATE KEY UPDATE data = VALUES(data), expires_at = VALUES(expires_at), last_activity = VALUES(last_activity), ip_address = VALUES(ip_address), user_agent = VALUES(user_agent)

-- auth_532 auth_select auth-framework@881c696d:src/storage/mysql.rs:532
SELECT session_id, user_id, created_at, expires_at, last_activity, ip_address, user_agent, data FROM sessions WHERE session_id = ?

-- auth_590 auth_delete auth-framework@881c696d:src/storage/mysql.rs:590
DELETE FROM sessions WHERE session_id = ?

-- auth_611 auth_insert auth-framework@881c696d:src/storage/mysql.rs:611
INSERT INTO kv_store (`key`, `value`, expires_at) VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE `value` = VALUES(`value`), expires_at = VALUES(expires_at)

-- auth_632 auth_select auth-framework@881c696d:src/storage/mysql.rs:632
SELECT `value` FROM kv_store WHERE `key` = ? AND (expires_at IS NULL OR expires_at > UTC_TIMESTAMP(6))

-- auth_645 auth_delete auth-framework@881c696d:src/storage/mysql.rs:645
DELETE FROM kv_store WHERE `key` = ?

-- auth_661 auth_select auth-framework@881c696d:src/storage/mysql.rs:661
SELECT `key` FROM kv_store WHERE `key` LIKE ? AND (expires_at IS NULL OR expires_at > UTC_TIMESTAMP(6)) ORDER BY `key`

-- auth_690 auth_delete auth-framework@881c696d:src/storage/mysql.rs:690
DELETE FROM auth_tokens WHERE expires_at < ?

-- auth_700 auth_delete auth-framework@881c696d:src/storage/mysql.rs:700
DELETE FROM sessions WHERE expires_at < ?

-- auth_710 auth_delete auth-framework@881c696d:src/storage/mysql.rs:710
DELETE FROM kv_store WHERE expires_at IS NOT NULL AND expires_at < ?

-- auth_725 auth_select auth-framework@881c696d:src/storage/mysql.rs:725
SELECT session_id, user_id, data, expires_at, created_at, last_activity, ip_address, user_agent FROM sessions WHERE user_id = ? AND (expires_at IS NULL OR expires_at > NOW())

-- auth_801 auth_select auth-framework@881c696d:src/storage/mysql.rs:801
SELECT COUNT(*) as count FROM sessions WHERE expires_at IS NULL OR expires_at > NOW()

