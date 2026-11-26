CREATE TABLE worldsmith_games (
 id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
 exe VARCHAR(260) NOT NULL,
 title VARCHAR(200) NOT NULL DEFAULT '',
 stamp VARCHAR(64) NOT NULL DEFAULT '',
 last_played DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
 UNIQUE KEY ux_worldsmith_games_exe_stamp (exe, stamp)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- one row per value the companion found in a game: how to find it again, and where it sits on the shot
CREATE TABLE worldsmith_values (
 id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
 game_id BIGINT UNSIGNED NOT NULL,
 label VARCHAR(60) NOT NULL,
 value_type VARCHAR(8) NOT NULL,
 transform VARCHAR(8) NOT NULL DEFAULT '',
 module VARCHAR(260) NOT NULL DEFAULT '',
 module_offset VARCHAR(20) NOT NULL DEFAULT '',
 last_address VARCHAR(20) NOT NULL DEFAULT '',
 found_how VARCHAR(255) NOT NULL DEFAULT '',
 pin_x DOUBLE NULL,
 pin_y DOUBLE NULL,
 updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
 UNIQUE KEY ux_worldsmith_values_game_label (game_id, label)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- a recipe the companion proposes waits for the member's Keep
CREATE TABLE worldsmith_recipes (
 id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
 game_id BIGINT UNSIGNED NOT NULL,
 name VARCHAR(80) NOT NULL,
 labels VARCHAR(255) NOT NULL,
 action VARCHAR(8) NOT NULL,
 amount VARCHAR(32) NOT NULL DEFAULT '',
 state VARCHAR(12) NOT NULL DEFAULT 'proposed',
 proposed_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
 decided_at DATETIME NULL,
 UNIQUE KEY ux_worldsmith_recipes_game_name (game_id, name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE worldsmith_sessions (
 id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
 game_id BIGINT UNSIGNED NOT NULL,
 steps VARCHAR(500) NOT NULL,
 at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
 KEY ix_worldsmith_sessions_game_at (game_id, at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
