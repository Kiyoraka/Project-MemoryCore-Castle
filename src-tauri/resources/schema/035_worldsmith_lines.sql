CREATE TABLE worldsmith_lines (
 id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
 game_id BIGINT UNSIGNED NOT NULL,
 source VARCHAR(300) NOT NULL,
 translated VARCHAR(400) NOT NULL,
 lang VARCHAR(10) NOT NULL DEFAULT 'en',
 seen_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
 UNIQUE KEY ux_worldsmith_lines_game_source_lang (game_id, source, lang)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- one row per piece of a game's on-screen text Translate read: the next read keeps its names the same
