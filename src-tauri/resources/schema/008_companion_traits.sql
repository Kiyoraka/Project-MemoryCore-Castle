-- The companion's emotion engine: her traits, one row each (the PROFILE tab and `castle-memory trait`).
-- number never changes once given, so a trait can always be cited as Trait N.
-- line is the one-line manner, the only part the chat room may hear, and only by audience:
-- everyone, owner (only in a line that answers the member) or never (not in the room at all).
-- why is private (the diary moment or the member's own words) and never leaves the memory.
-- status: proposed (waiting for the member's yes), active (she lives it), retired (kept, not lived).

CREATE TABLE companion_traits (
  id            BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
  number        INT UNSIGNED NOT NULL,
  name          VARCHAR(80)  NOT NULL,
  line          VARCHAR(200) NOT NULL,
  body          TEXT         NOT NULL,
  audience      VARCHAR(10)  NOT NULL DEFAULT 'everyone',
  origin        VARCHAR(10)  NOT NULL DEFAULT 'written',
  why           TEXT         NULL,
  status        VARCHAR(10)  NOT NULL DEFAULT 'proposed',
  created_at    DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at    DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  UNIQUE KEY uq_trait_number (number),
  KEY ix_trait_status (status, number)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE companion_trait_history (
  id            BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
  trait_id      BIGINT UNSIGNED NOT NULL,
  name          VARCHAR(80)  NOT NULL,
  line          VARCHAR(200) NOT NULL,
  body          TEXT         NOT NULL,
  audience      VARCHAR(10)  NOT NULL,
  status        VARCHAR(10)  NOT NULL,
  replaced_at   DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY ix_trait_history (trait_id, replaced_at),
  CONSTRAINT fk_trait_history_trait FOREIGN KEY (trait_id) REFERENCES companion_traits (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
