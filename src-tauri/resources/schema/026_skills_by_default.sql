ALTER TABLE skills ALTER COLUMN stubbed SET DEFAULT 1;
UPDATE skills SET stubbed = 1;

-- Every skill is served from the database by default: its SKILL.md on disk becomes a stub that
-- loads the instructions when the skill fires (fewer tokens, nothing drifts). The UPDATE runs
-- once, as this migration does: a skill the member switches back to a whole file stays whole.
