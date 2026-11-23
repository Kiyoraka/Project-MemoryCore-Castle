CREATE TABLE skill_flows (
  plugin        VARCHAR(100) NOT NULL,
  name          VARCHAR(100) NOT NULL,
  flow          VARCHAR(64)  NOT NULL,
  sections      TEXT         NOT NULL,
  note          VARCHAR(255) NULL,
  updated_at    DATETIME     NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  PRIMARY KEY (plugin, name, flow)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

-- A skill's flows: a name for one moment the skill serves and the sections that moment needs
-- (their keys, in order, one per line), so a firing loads only those - `skill show --for <flow>`.
