-- Exact pending counts need only the KB key. Keeping timestamps in the separate
-- queue index lets this index compact repeated KB keys into B-tree posting lists.
CREATE INDEX IF NOT EXISTS resolution_reviews_pending_count_idx
    ON resolution_reviews (kb_id) WHERE status = 'pending';
