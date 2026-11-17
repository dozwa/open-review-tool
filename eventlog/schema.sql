-- Event-Log für Screening-Entscheidungen (Entwurf v0.1, PostgreSQL 16)
CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE review (
  review_id   uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  title       text NOT NULL,
  last_seq    bigint NOT NULL DEFAULT 0
);

CREATE TABLE event (
  review_id      uuid        NOT NULL REFERENCES review,
  seq            bigint      NOT NULL,          -- fortlaufend pro Review
  event_id       uuid        NOT NULL DEFAULT gen_random_uuid() UNIQUE,
  type           text        NOT NULL,
  schema_version smallint    NOT NULL DEFAULT 1,
  actor_id       uuid        NOT NULL,          -- pseudonym, Klarname in separater Tabelle
  stage_instance_id uuid,
  occurred_at    timestamptz NOT NULL DEFAULT now(),
  payload        jsonb       NOT NULL,
  prev_hash      bytea,                          -- Hash des Vorgängers (Kette)
  hash           bytea       NOT NULL,
  PRIMARY KEY (review_id, seq)
);

-- Anhängen mit optimistischer Nebenläufigkeit: Aufrufer nennt die erwartete letzte seq
CREATE FUNCTION append_event(p_review uuid, p_expected_seq bigint, p_type text,
                             p_actor uuid, p_stage uuid, p_payload jsonb)
RETURNS bigint LANGUAGE plpgsql AS $$
DECLARE v_seq bigint; v_prev bytea;
BEGIN
  UPDATE review SET last_seq = last_seq + 1
   WHERE review_id = p_review AND last_seq = p_expected_seq
   RETURNING last_seq INTO v_seq;
  IF v_seq IS NULL THEN
    RAISE EXCEPTION 'concurrency conflict: expected seq % is stale', p_expected_seq;
  END IF;
  SELECT hash INTO v_prev FROM event WHERE review_id = p_review AND seq = v_seq - 1;
  INSERT INTO event (review_id, seq, type, actor_id, stage_instance_id, payload, prev_hash, hash)
  VALUES (p_review, v_seq, p_type, p_actor, p_stage, p_payload, v_prev,
          digest(coalesce(v_prev, ''::bytea) || convert_to(p_type || p_payload::text, 'UTF8'), 'sha256'));
  RETURN v_seq;
END $$;

-- Unveränderlichkeit erzwingen
CREATE FUNCTION forbid_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'event log is append-only'; END $$;
CREATE TRIGGER event_immutable BEFORE UPDATE OR DELETE ON event
  FOR EACH ROW EXECUTE FUNCTION forbid_change();

-- Snapshot = benannter Stand bis seq; Prüfsumme = Hash des letzten Ereignisses
CREATE TABLE snapshot (
  snapshot_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  review_id   uuid NOT NULL REFERENCES review,
  label       text NOT NULL,
  up_to_seq   bigint NOT NULL,
  hash        bytea NOT NULL,
  FOREIGN KEY (review_id, up_to_seq) REFERENCES event (review_id, seq)
);

-- Beispielprojektion: jeweils gültiges Votum pro Person, Record und Stufeninstanz
CREATE VIEW current_vote AS
SELECT DISTINCT ON (e.review_id, e.stage_instance_id, e.payload->>'record_id', e.actor_id)
       e.review_id, e.stage_instance_id, e.payload->>'record_id' AS record_id,
       e.actor_id, e.payload->>'decision' AS decision, e.seq
  FROM event e
 WHERE e.type IN ('VoteCast', 'VoteRevised')
 ORDER BY e.review_id, e.stage_instance_id, e.payload->>'record_id', e.actor_id, e.seq DESC;

-- Beispielprojektion: Konflikte (Include vs. Exclude) pro Stufeninstanz
CREATE VIEW conflict AS
SELECT review_id, stage_instance_id, record_id
  FROM current_vote
 GROUP BY review_id, stage_instance_id, record_id
HAVING bool_or(decision = 'include') AND bool_or(decision = 'exclude');
