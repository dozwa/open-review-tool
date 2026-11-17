-- Härtung des Event-Logs (v0.3)

-- TRUNCATE umgeht Zeilen-Trigger; deshalb zusätzlich auf Anweisungsebene verbieten.
CREATE TRIGGER event_no_truncate BEFORE TRUNCATE ON event
  FOR EACH STATEMENT EXECUTE FUNCTION forbid_change();

-- Prüft die Hash-Kette eines Reviews: lückenlose seq, prev_hash = Hash des Vorgängers,
-- hash = sha256(prev_hash || type || payload). n_bad = 0 heißt unversehrt.
CREATE FUNCTION verify_chain(p_review uuid)
RETURNS TABLE (n_events bigint, n_bad bigint, first_bad_seq bigint)
LANGUAGE sql STABLE AS $$
  WITH e AS (
    SELECT seq,
           seq <> row_number() OVER (ORDER BY seq)
           OR prev_hash IS DISTINCT FROM lag(hash) OVER (ORDER BY seq)
           OR hash <> sha256(coalesce(prev_hash, ''::bytea) || convert_to(type || payload::text, 'UTF8')) AS bad
      FROM event WHERE review_id = p_review
  )
  SELECT count(*), count(*) FILTER (WHERE bad), min(seq) FILTER (WHERE bad) FROM e
$$;
