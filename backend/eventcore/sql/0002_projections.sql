-- Projektionen final_decision und agreement (Entwurf v0.2, PostgreSQL 16)
-- Setzt schema.sql voraus (Tabellen event, review; View current_vote).
-- v0.2: Maybe-Regel aus StageInstanceOpened, Krippendorffs Alpha, Indizes,
--       Cohen-Paare ohne korrelierte Unterabfragen.

-- Indizes für Projektionen pro Stufeninstanz
CREATE INDEX event_stage_type_idx ON event (stage_instance_id, type);
CREATE INDEX event_record_idx ON event ((payload->>'record_id'));

-- Stufenkonfiguration: es gilt das ERSTE StageInstanceOpened, damit Regeln wie
-- min_reviewers und maybe_as nach Beginn der Stufe nicht mehr änderbar sind.
-- maybe_as: NULL = Maybe ist eigene Kategorie; 'include' / 'exclude' = Zuordnung.
CREATE VIEW stage_config AS
SELECT DISTINCT ON (review_id, stage_instance_id)
       review_id, stage_instance_id,
       coalesce((payload->>'min_reviewers')::int, 2) AS min_reviewers,
       payload->>'maybe_as'                          AS maybe_as,
       payload->>'criteria_version'                  AS criteria_version
  FROM event
 WHERE type = 'StageInstanceOpened'
 ORDER BY review_id, stage_instance_id, seq ASC;

-- Gültige Voten mit angewandter Maybe-Regel der Stufe
CREATE VIEW current_vote_mapped AS
SELECT cv.review_id, cv.stage_instance_id, cv.record_id, cv.actor_id, cv.seq,
       cv.decision AS raw_decision,
       CASE WHEN cv.decision = 'maybe' AND s.maybe_as IS NOT NULL
            THEN s.maybe_as ELSE cv.decision END AS decision
  FROM current_vote cv
  LEFT JOIN stage_config s
    ON s.review_id = cv.review_id AND s.stage_instance_id = cv.stage_instance_id;

-- Letzter Konsens pro Record und Stufeninstanz
CREATE VIEW current_consensus AS
SELECT DISTINCT ON (review_id, stage_instance_id, payload->>'record_id')
       review_id, stage_instance_id, payload->>'record_id' AS record_id,
       payload->>'decision' AS decision, payload->>'method' AS method, seq
  FROM event
 WHERE type = 'ConsensusRecorded'
 ORDER BY review_id, stage_instance_id, payload->>'record_id', seq DESC;

-- Records, die jemand als "nicht entscheidbar" markiert hat (z. B. Abstract fehlt,
-- Sprache unklar). Zählt nicht als Votum, schickt den Record in die Klärung.
CREATE VIEW undecidable_flag AS
SELECT review_id, stage_instance_id, payload->>'record_id' AS record_id,
       max(seq) AS last_seq, count(*) AS n_flags
  FROM event
 WHERE type = 'RecordFlaggedUndecidable'
 GROUP BY review_id, stage_instance_id, payload->>'record_id';

-- final_decision: Konsens schlägt Einzelvoten und Markierungen; sonst einstimmig ab Mindestzahl.
-- Verwendet die Maybe-Regel der Stufe.
-- status: consensus | needs_clarification | unanimous | pending | disagreement
CREATE VIEW final_decision AS
WITH v AS (
  SELECT review_id, stage_instance_id, record_id,
         count(*)                 AS n_votes,
         count(DISTINCT decision) AS n_distinct,
         min(decision)            AS any_decision
    FROM current_vote_mapped
   GROUP BY review_id, stage_instance_id, record_id
)
, keys AS (
  SELECT review_id, stage_instance_id, record_id FROM v
  UNION SELECT review_id, stage_instance_id, record_id FROM current_consensus
  UNION SELECT review_id, stage_instance_id, record_id FROM undecidable_flag
)
SELECT k.review_id, k.stage_instance_id, k.record_id,
       coalesce(v.n_votes, 0) AS n_votes,
       CASE
         WHEN c.record_id IS NOT NULL THEN c.decision
         WHEN u.record_id IS NOT NULL THEN NULL
         WHEN v.n_distinct = 1 AND v.n_votes >= coalesce(s.min_reviewers, 2) THEN v.any_decision
       END AS decision,
       CASE
         WHEN c.record_id IS NOT NULL THEN 'consensus'
         WHEN u.record_id IS NOT NULL THEN 'needs_clarification'
         WHEN v.n_distinct > 1 THEN 'disagreement'
         WHEN coalesce(v.n_votes, 0) < coalesce(s.min_reviewers, 2) THEN 'pending'
         ELSE 'unanimous'
       END AS status
  FROM keys k
  LEFT JOIN v USING (review_id, stage_instance_id, record_id)
  LEFT JOIN current_consensus c USING (review_id, stage_instance_id, record_id)
  LEFT JOIN undecidable_flag u USING (review_id, stage_instance_id, record_id)
  LEFT JOIN stage_config s USING (review_id, stage_instance_id);

-- Voten einer Stufeninstanz. p_maybe_as:
--   '__stage__' (Standard) = Regel aus dem Protokoll der Stufe
--   NULL                   = Maybe als eigene Kategorie (explizite Abweichung, z. B. Sensitivitätsanalyse)
--   'include'/'exclude'    = explizite Zuordnung
CREATE FUNCTION stage_votes(p_stage uuid, p_maybe_as text DEFAULT '__stage__')
RETURNS TABLE (record_id text, actor_id uuid, decision text)
LANGUAGE sql STABLE AS $$
  SELECT cv.record_id, cv.actor_id,
         CASE WHEN p_maybe_as = '__stage__' THEN cv.decision
              WHEN cv.raw_decision = 'maybe' AND p_maybe_as IS NOT NULL THEN p_maybe_as
              ELSE cv.raw_decision END
    FROM current_vote_mapped cv
   WHERE cv.stage_instance_id = p_stage
$$;

-- Prozentuale Übereinstimmung: Anteil der Records mit >= 2 Voten, bei denen alle gleich sind
CREATE FUNCTION percent_agreement(p_stage uuid, p_maybe_as text DEFAULT '__stage__')
RETURNS TABLE (n_items int, percent_agreement double precision)
LANGUAGE sql STABLE AS $$
  SELECT count(*)::int,
         avg(CASE WHEN nd = 1 THEN 1.0 ELSE 0.0 END)::double precision
    FROM (SELECT record_id, count(DISTINCT decision) AS nd
            FROM stage_votes(p_stage, p_maybe_as)
           GROUP BY record_id HAVING count(*) >= 2) x
$$;

-- Fleiss' Kappa über alle Records mit genau n Voten (n = Mindestzahl der Stufe).
-- Records mit anderer Votenzahl fallen heraus -> dafür krippendorff_alpha verwenden.
CREATE FUNCTION fleiss_kappa(p_stage uuid, p_maybe_as text DEFAULT '__stage__', p_n int DEFAULT NULL)
RETURNS TABLE (n_items int, n_items_excluded int, raters_per_item int,
               p_bar double precision, p_e double precision, kappa double precision)
LANGUAGE plpgsql STABLE AS $$
DECLARE v_n int;
BEGIN
  v_n := coalesce(p_n, (SELECT min_reviewers FROM stage_config WHERE stage_instance_id = p_stage LIMIT 1), 2);
  RETURN QUERY
  WITH votes AS (SELECT * FROM stage_votes(p_stage, p_maybe_as)),
  per_item AS (SELECT record_id, count(*) AS m FROM votes GROUP BY record_id),
  items AS (SELECT record_id FROM per_item WHERE m = v_n),
  nij AS (SELECT v.record_id, v.decision, count(*)::double precision AS c
            FROM votes v JOIN items i USING (record_id)
           GROUP BY v.record_id, v.decision),
  pi AS (SELECT record_id, (sum(c * c) - v_n) / (v_n * (v_n - 1.0)) AS p
           FROM nij GROUP BY record_id),
  pj AS (SELECT decision, sum(c) / ((SELECT count(*) FROM items) * v_n) AS p
           FROM nij GROUP BY decision),
  agg AS (SELECT (SELECT count(*) FROM items)::int AS n_items,
                 (SELECT count(*) FROM per_item WHERE m >= 2 AND m <> v_n)::int AS n_excl,
                 (SELECT avg(p) FROM pi) AS p_bar,
                 (SELECT sum(p * p) FROM pj) AS p_e)
  SELECT agg.n_items, agg.n_excl, v_n, agg.p_bar, agg.p_e,
         CASE WHEN agg.p_e < 1 THEN (agg.p_bar - agg.p_e) / (1 - agg.p_e) END
    FROM agg;
END $$;

-- Krippendorffs Alpha (nominal) über alle Records mit >= 2 Voten, beliebige Votenzahl.
-- alpha = 1 - (n - 1) * sum_u (m_u^2 - sum_c n_uc^2) / (m_u - 1)  /  (n^2 - sum_c n_c^2)
-- (Krippendorff 2011, Coincidence-Matrix-Form)
CREATE FUNCTION krippendorff_alpha(p_stage uuid, p_maybe_as text DEFAULT '__stage__')
RETURNS TABLE (n_items int, n_pairable_values int, d_o double precision,
               d_e double precision, alpha double precision)
LANGUAGE sql STABLE AS $$
  WITH votes AS (SELECT * FROM stage_votes(p_stage, p_maybe_as)),
  nuc AS (SELECT record_id, decision, count(*)::double precision AS c
            FROM votes GROUP BY record_id, decision),
  mu AS (SELECT record_id, sum(c) AS m, sum(c * c) AS sq FROM nuc GROUP BY record_id HAVING sum(c) >= 2),
  nc AS (SELECT n.decision, sum(n.c) AS c FROM nuc n JOIN mu USING (record_id) GROUP BY n.decision),
  agg AS (SELECT (SELECT count(*) FROM mu)::int AS n_items,
                 (SELECT sum(m) FROM mu) AS n,
                 (SELECT sum((m * m - sq) / (m - 1)) FROM mu) AS disagree,
                 (SELECT sum(c * c) FROM nc) AS sq_c)
  SELECT n_items, n::int,
         disagree / n,
         (n * n - sq_c) / (n * (n - 1)),
         CASE WHEN n * n - sq_c > 0 THEN 1 - (n - 1) * disagree / (n * n - sq_c) END
    FROM agg
$$;

-- Cohen's Kappa für jedes Reviewer-Paar auf den gemeinsam bewerteten Records,
-- plus PABAK (Byrt et al. 1993): (k * p_o - 1) / (k - 1), bei 2 Kategorien = 2 p_o - 1.
-- k = Zahl der Kategorien, die in der Stufe vorkommen.
CREATE FUNCTION cohen_kappa_pairs(p_stage uuid, p_maybe_as text DEFAULT '__stage__')
RETURNS TABLE (rater_a uuid, rater_b uuid, n_items int, p_o double precision,
               p_e double precision, kappa double precision, pabak double precision)
LANGUAGE sql STABLE AS $$
  WITH votes AS (SELECT * FROM stage_votes(p_stage, p_maybe_as)),
  kc AS (SELECT count(DISTINCT decision)::double precision AS k FROM votes),
  pairs AS (
    SELECT a.actor_id AS ra, b.actor_id AS rb, a.decision AS da, b.decision AS db
      FROM votes a JOIN votes b
        ON a.record_id = b.record_id AND a.actor_id < b.actor_id
  ),
  n AS (SELECT ra, rb, count(*)::double precision AS n,
               avg(CASE WHEN da = db THEN 1.0 ELSE 0.0 END) AS p_o
          FROM pairs GROUP BY ra, rb),
  ma AS (SELECT ra, rb, da AS k, count(*)::double precision AS c FROM pairs GROUP BY ra, rb, da),
  mb AS (SELECT ra, rb, db AS k, count(*)::double precision AS c FROM pairs GROUP BY ra, rb, db),
  pe AS (SELECT ma.ra, ma.rb, sum(ma.c * mb.c) AS s
           FROM ma JOIN mb ON ma.ra = mb.ra AND ma.rb = mb.rb AND ma.k = mb.k
          GROUP BY ma.ra, ma.rb)
  SELECT n.ra, n.rb, n.n::int, n.p_o::double precision,
         coalesce(pe.s, 0) / (n.n * n.n),
         CASE WHEN coalesce(pe.s, 0) / (n.n * n.n) < 1
              THEN (n.p_o - coalesce(pe.s, 0) / (n.n * n.n)) / (1 - coalesce(pe.s, 0) / (n.n * n.n)) END,
         CASE WHEN kc.k > 1 THEN (kc.k * n.p_o - 1) / (kc.k - 1) END
    FROM n LEFT JOIN pe USING (ra, rb) CROSS JOIN kc
$$;

-- Votenmatrix für die Anwendungsschicht (Bootstrap-Konfidenzintervalle in agreement.py)
CREATE FUNCTION vote_matrix(p_stage uuid, p_maybe_as text DEFAULT '__stage__')
RETURNS TABLE (record_id text, actor_id uuid, decision text)
LANGUAGE sql STABLE AS $$
  SELECT * FROM stage_votes(p_stage, p_maybe_as) ORDER BY 1, 2
$$;
