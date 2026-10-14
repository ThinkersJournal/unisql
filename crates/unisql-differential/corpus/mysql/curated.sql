-- SPDX-License-Identifier: MIT OR Apache-2.0
-- Curated MySQL/MariaDB DML: the idioms that application code written for MySQL uses and that
-- are not in any corpus project. Not taken from any third-party source: written for this
-- file from the MySQL 8.4 and MariaDB 11.4 reference manuals' statement syntax. One
-- statement per block; a block is a header line `-- <id> <group>` and SQL up to a blank line.
-- Tables are those of `schema.sql`. `?` is a placeholder.

-- q_backtick quoting
SELECT `id`, `name` FROM `users` WHERE `active` = 1

-- q_double_quoted_string quoting
SELECT id FROM users WHERE name = "alice"

-- limit_offset_comma paging
SELECT id, name FROM users ORDER BY id LIMIT 2, 3

-- limit_offset_keyword paging
SELECT id, name FROM users ORDER BY id LIMIT 3 OFFSET 2

-- limit_param paging
SELECT id FROM users ORDER BY id LIMIT ?, ?

-- upsert_values upsert
INSERT INTO kv (k, v, n) VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE v = VALUES(v), n = n + 1

-- upsert_alias upsert
INSERT INTO kv (k, v, n) VALUES (?, ?, ?) AS new ON DUPLICATE KEY UPDATE v = new.v

-- insert_ignore upsert
INSERT IGNORE INTO kv (k, v, n) VALUES (?, ?, ?)

-- insert_set insert
INSERT INTO kv SET k = ?, v = ?, n = ?

-- replace_into upsert
REPLACE INTO kv (k, v, n) VALUES (?, ?, ?)

-- insert_select insert
INSERT INTO kv (k, v, n) SELECT name, email, age FROM users WHERE active = 1

-- insert_multi insert
INSERT INTO kv (k, v, n) VALUES ('a', 'x', 1), ('b', 'y', 2), ('c', NULL, 3)

-- insert_default insert
INSERT INTO kv (k, v) VALUES (?, DEFAULT)

-- update_limit update
UPDATE users SET active = 0 WHERE age > 50 ORDER BY id LIMIT 10

-- update_join update
UPDATE users u JOIN orders o ON o.user_id = u.id SET u.score = u.score + 1 WHERE o.status = 'paid'

-- update_low_priority update
UPDATE LOW_PRIORITY users SET score = 0 WHERE id = ?

-- delete_limit delete
DELETE FROM kv WHERE n IS NULL ORDER BY k LIMIT 5

-- delete_join delete
DELETE o FROM orders o JOIN users u ON u.id = o.user_id WHERE u.active = 0

-- delete_quick delete
DELETE QUICK FROM kv WHERE k = ?

-- truncate delete
TRUNCATE TABLE kv

-- ifnull function
SELECT IFNULL(email, 'none') FROM users

-- if_function function
SELECT IF(active = 1, 'on', 'off') FROM users

-- concat function
SELECT CONCAT(name, ' <', email, '>') FROM users

-- concat_ws function
SELECT CONCAT_WS(', ', name, city) FROM users

-- group_concat function
SELECT user_id, GROUP_CONCAT(status ORDER BY id SEPARATOR ',') FROM orders GROUP BY user_id

-- group_concat_distinct function
SELECT GROUP_CONCAT(DISTINCT status) FROM orders

-- now function
SELECT NOW(), CURRENT_TIMESTAMP(), CURDATE()

-- utc_timestamp function
SELECT UTC_TIMESTAMP(), UTC_DATE()

-- date_add function
SELECT DATE_ADD(NOW(), INTERVAL 7 DAY), DATE_SUB(NOW(), INTERVAL 1 HOUR)

-- interval_arith function
SELECT id FROM events WHERE at > NOW() - INTERVAL 1 DAY

-- date_format function
SELECT DATE_FORMAT(at, '%Y-%m-%d') FROM events

-- unix_timestamp function
SELECT UNIX_TIMESTAMP(at), FROM_UNIXTIME(1592228730) FROM events

-- datediff function
SELECT DATEDIFF(NOW(), at) FROM events

-- timestampdiff function
SELECT TIMESTAMPDIFF(SECOND, at, NOW()) FROM events

-- date_func function
SELECT DATE(at), YEAR(at), MONTH(at), DAY(at), HOUR(at) FROM events

-- last_insert_id function
SELECT LAST_INSERT_ID()

-- found_rows function
SELECT SQL_CALC_FOUND_ROWS id FROM users LIMIT 2

-- row_count function
SELECT ROW_COUNT()

-- cast_signed function
SELECT CAST(v AS SIGNED), CAST(v AS UNSIGNED) FROM kv

-- cast_char function
SELECT CAST(id AS CHAR), CONVERT(id, CHAR) FROM users

-- cast_decimal function
SELECT CAST(score AS DECIMAL(10, 2)) FROM users

-- cast_date function
SELECT CAST(at AS DATE) FROM events

-- coalesce function
SELECT COALESCE(email, city, 'x') FROM users

-- nullif function
SELECT NULLIF(status, 'void') FROM orders

-- case_expr expression
SELECT CASE status WHEN 'paid' THEN 1 WHEN 'open' THEN 0 ELSE -1 END FROM orders

-- mod_div expression
SELECT 7 DIV 2, 7 MOD 2, 7 % 2, 7 / 2

-- xor expression
SELECT a.id FROM users a WHERE (a.active = 1) XOR (a.age > 30)

-- bit_ops expression
SELECT id & 3, id | 4, id ^ 1, ~id, id << 1, id >> 1 FROM users

-- regexp expression
SELECT id FROM users WHERE name REGEXP '^a'

-- rlike expression
SELECT id FROM users WHERE name RLIKE 'e$'

-- like_binary expression
SELECT id FROM users WHERE name LIKE BINARY 'a%'

-- like_escape expression
SELECT id FROM users WHERE name LIKE 'a\\_%' ESCAPE '\\'

-- hex_literal expression
SELECT 0x41, X'41', b'101'

-- user_variable expression
SELECT @total := COUNT(*) FROM users

-- null_safe expression
SELECT id FROM users WHERE city <=> ?

-- is_true expression
SELECT id FROM users WHERE (age > 30) IS TRUE

-- json_extract json
SELECT JSON_EXTRACT(meta, '$.a') FROM users

-- json_arrow json
SELECT meta->'$.a', meta->>'$.b' FROM users

-- json_unquote json
SELECT JSON_UNQUOTE(JSON_EXTRACT(meta, '$.a')) FROM users

-- json_object json
SELECT JSON_OBJECT('id', id, 'name', name) FROM users

-- json_array_agg json
SELECT JSON_ARRAYAGG(name) FROM users

-- match_against fulltext
SELECT id FROM users WHERE MATCH(name) AGAINST('alice')

-- for_update locking
SELECT id FROM users WHERE id = ? FOR UPDATE

-- lock_in_share_mode locking
SELECT id FROM users WHERE id = ? LOCK IN SHARE MODE

-- for_update_skip_locked locking
SELECT id FROM orders WHERE status = 'open' ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED

-- straight_join join
SELECT STRAIGHT_JOIN u.id FROM users u JOIN orders o ON o.user_id = u.id

-- use_index join
SELECT id FROM users USE INDEX (PRIMARY) WHERE id = 1

-- natural_join join
SELECT * FROM users NATURAL JOIN orders

-- join_using join
SELECT u.id FROM users u JOIN orders o USING (id)

-- cross_join join
SELECT u.id, o.id FROM users u CROSS JOIN orders o

-- derived_table subquery
SELECT t.c FROM (SELECT COUNT(*) AS c FROM orders GROUP BY user_id) AS t

-- cte cte
WITH t AS (SELECT user_id, SUM(amount) AS s FROM orders GROUP BY user_id) SELECT user_id FROM t WHERE s > 100

-- cte_recursive cte
WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 5) SELECT i FROM n

-- window_row_number window
SELECT id, ROW_NUMBER() OVER (PARTITION BY city ORDER BY id) FROM users

-- window_sum window
SELECT id, SUM(amount) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM orders

-- group_with_rollup aggregate
SELECT user_id, SUM(amount) FROM orders GROUP BY user_id WITH ROLLUP

-- group_by_alias aggregate
SELECT user_id AS u, COUNT(*) FROM orders GROUP BY u HAVING COUNT(*) > 1

-- union_all setop
SELECT id FROM users UNION ALL SELECT user_id FROM orders

-- intersect setop
SELECT id FROM users INTERSECT SELECT user_id FROM orders

-- except setop
SELECT id FROM users EXCEPT SELECT user_id FROM orders

-- in_subquery subquery
SELECT id FROM users WHERE id IN (SELECT user_id FROM orders WHERE status = 'paid')

-- exists_subquery subquery
SELECT id FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.id)

-- select_into_outfile unsupported
SELECT id FROM users INTO OUTFILE '/tmp/x'

-- values_row row
SELECT * FROM (VALUES ROW(1, 'a'), ROW(2, 'b')) AS t(i, s)

-- table_stmt row
TABLE users

-- select_distinct_row_count aggregate
SELECT COUNT(DISTINCT city), MAX(age), MIN(age), AVG(score) FROM users

-- order_by_field ordering
SELECT id FROM users ORDER BY FIELD(city, 'Oslo', 'Bergen'), id

-- order_by_null ordering
SELECT id FROM users ORDER BY age IS NULL, age

-- rand ordering
SELECT id FROM users ORDER BY RAND() LIMIT 1

-- locate string
SELECT LOCATE('a', name), INSTR(name, 'a'), POSITION('a' IN name) FROM users

-- substring_forms string
SELECT SUBSTRING(name, 2), SUBSTRING(name, 2, 3), SUBSTRING(name FROM 2 FOR 3), SUBSTR(name, 1, 1) FROM users

-- string_misc string
SELECT LEFT(name, 2), RIGHT(name, 2), LPAD(name, 8, '*'), REPEAT('a', 3), REVERSE(name), CHAR_LENGTH(name) FROM users

-- trim_forms string
SELECT TRIM(name), LTRIM(name), RTRIM(name), TRIM(BOTH 'x' FROM name) FROM users

-- upper_lower string
SELECT UPPER(name), LOWER(name), LENGTH(name) FROM users

-- replace_func string
SELECT REPLACE(name, 'a', 'b') FROM users

-- md5_sha string
SELECT MD5(name), SHA1(name), SHA2(name, 256) FROM users

-- uuid_func function
SELECT UUID()

-- math_funcs function
SELECT ABS(-1), CEIL(1.2), FLOOR(1.8), ROUND(1.567, 2), TRUNCATE(1.567, 1), POW(2, 3), SQRT(4), GREATEST(1, 2), LEAST(1, 2)

-- sleep_func function
SELECT SLEEP(0)

-- version_func function
SELECT VERSION(), DATABASE(), CURRENT_USER(), CONNECTION_ID()

-- set_variable session
SET @x = 1

-- set_names session
SET NAMES utf8mb4

-- show_tables session
SHOW TABLES

-- explain session
EXPLAIN SELECT id FROM users

-- use_db session
USE uni

-- start_transaction session
START TRANSACTION

-- commit session
COMMIT

-- savepoint session
SAVEPOINT s1

-- call_proc session
CALL some_proc(1)

-- insert_returning returning
INSERT INTO kv (k, v, n) VALUES (?, ?, ?) RETURNING k

-- delete_returning returning
DELETE FROM kv WHERE k = ? RETURNING k, v
