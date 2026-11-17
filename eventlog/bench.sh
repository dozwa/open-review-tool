#!/bin/bash
# Lasttest: 20.000 Records x 2 Reviewer (Titel/Abstract) und 5.000 Records x 3 Reviewer
set -e
P=/usr/lib/postgresql/16/bin; T=$(mktemp -d); chmod 777 $T
RUN="su nobody -s /bin/bash -c"; [ "$(id -u)" != "0" ] && RUN="bash -c"
$RUN "$P/initdb -D $T/data -A trust -U pg >/dev/null && $P/pg_ctl -D $T/data -o '-k $T -p 5497 -c listen_addresses=' -l $T/log -w start >/dev/null"
cat schema.sql projections.sql > $T/setup.sql
cat > $T/load.sql <<'SQL'
INSERT INTO review (review_id, title) VALUES ('00000000-0000-0000-0000-000000000001','Bench');
DO $$
DECLARE s uuid; n int; r int; i int; rv uuid := '00000000-0000-0000-0000-000000000001';
        sys uuid := '00000000-0000-0000-0009-000000000000'; d text; seqv bigint;
BEGIN
  PERFORM setseed(0.42);
  FOREACH n IN ARRAY ARRAY[2,3] LOOP
    s := ('00000000-0000-0000-0001-00000000000' || n)::uuid;
    SELECT last_seq INTO seqv FROM review WHERE review_id = rv;
    PERFORM append_event(rv, seqv, 'StageInstanceOpened', sys, s, jsonb_build_object('min_reviewers', n));
    FOR r IN 1 .. CASE WHEN n = 2 THEN 20000 ELSE 5000 END LOOP
      FOR i IN 1 .. n LOOP
        d := CASE WHEN random() < 0.08 THEN 'include' WHEN random() < 0.05 THEN 'maybe' ELSE 'exclude' END;
        seqv := seqv + 1;
        PERFORM append_event(rv, seqv, 'VoteCast', ('00000000-0000-0000-0003-00000000000' || i)::uuid, s,
                             jsonb_build_object('record_id', n || '-' || r, 'decision', d));
      END LOOP;
    END LOOP;
  END LOOP;
END $$;
ANALYZE event;
SQL
cat > $T/q.sql <<'SQL'
\timing on
\echo == Events
SELECT count(*) FROM event;
\echo == final_decision (20k x 2)
SELECT status, count(*) FROM final_decision WHERE stage_instance_id='00000000-0000-0000-0001-000000000002' GROUP BY 1 ORDER BY 1;
\echo == percent_agreement (20k x 2)
SELECT * FROM percent_agreement('00000000-0000-0000-0001-000000000002');
\echo == cohen_kappa_pairs (20k x 2)
SELECT n_items, round(kappa::numeric,4), round(pabak::numeric,4) FROM cohen_kappa_pairs('00000000-0000-0000-0001-000000000002');
\echo == fleiss_kappa (5k x 3)
SELECT n_items, round(kappa::numeric,4) FROM fleiss_kappa('00000000-0000-0000-0001-000000000003');
\echo == krippendorff_alpha (20k x 2)
SELECT n_items, round(alpha::numeric,4) FROM krippendorff_alpha('00000000-0000-0000-0001-000000000002');
\echo == cohen_kappa_pairs (5k x 3, 3 Paare)
SELECT count(*) FROM cohen_kappa_pairs('00000000-0000-0000-0001-000000000003');
SQL
chmod 644 $T/*.sql
$RUN "$P/psql -h $T -p 5497 -U pg -d postgres -q -v ON_ERROR_STOP=1 -f $T/setup.sql" >/dev/null
START=$(date +%s.%N)
$RUN "$P/psql -h $T -p 5497 -U pg -d postgres -q -v ON_ERROR_STOP=1 -f $T/load.sql"
echo "Laden (55.002 Events über append_event): $(echo "$(date +%s.%N) - $START" | bc) s"
$RUN "$P/psql -h $T -p 5497 -U pg -d postgres -v ON_ERROR_STOP=1 -f $T/q.sql"
$RUN "$P/psql -h $T -p 5497 -U pg -d postgres -At -c \"COPY (SELECT record_id, actor_id, decision FROM vote_matrix('00000000-0000-0000-0001-000000000002')) TO STDOUT WITH CSV\"" > $T/votes.csv
python3 - "$T/votes.csv" <<'PY'
import csv, sys, time, agreement as ag
v = [tuple(r) for r in csv.reader(open(sys.argv[1]))]
raters = sorted({r[1] for r in v})
for m in ["percent", "alpha", "cohen"]:
    t = time.time(); e = ag.bootstrap(v, m, n_boot=2000, pair=tuple(raters) if m == "cohen" else None)
    print(f"Bootstrap {m:7s} B=2000, 20k Records: {e.value:.4f} [{e.ci_low:.4f}, {e.ci_high:.4f}] in {time.time()-t:.1f} s")
PY
$RUN "$P/pg_ctl -D $T/data -m fast stop >/dev/null"; rm -rf $T
