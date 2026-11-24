CREATE TABLE finance_receipts (
 id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
 state VARCHAR(12) NOT NULL DEFAULT 'proposed',
 shop VARCHAR(120) NOT NULL,
 total_cents BIGINT NOT NULL,
 currency CHAR(3) NOT NULL,
 spent_on DATE NOT NULL,
 category VARCHAR(24) NOT NULL,
 account_last4 CHAR(4) NOT NULL DEFAULT '',
 note VARCHAR(255) NOT NULL DEFAULT '',
 picture_path VARCHAR(500) NOT NULL DEFAULT '',
 picture_sha256 CHAR(64) NOT NULL,
 source VARCHAR(12) NOT NULL DEFAULT 'chat',
 proposed_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
 decided_at DATETIME NULL,
 UNIQUE KEY ux_finance_receipts_picture (picture_sha256),
 KEY ix_finance_receipts_state_spent (state, spent_on)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
