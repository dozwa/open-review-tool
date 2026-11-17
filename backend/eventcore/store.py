"""Einzige Schreibstelle für das Event-Log."""
import json

from django.db import DatabaseError, connection, transaction


class ConcurrencyError(Exception):
    pass


def append_expected(review_id, expected_seq, type, actor_id, payload, stage_id=None):
    """Hängt ein Ereignis an, wenn expected_seq noch die letzte Sequenznummer ist."""
    try:
        with transaction.atomic():
            with connection.cursor() as cur:
                cur.execute(
                    "SELECT append_event(%s::uuid, %s::bigint, %s::text, %s::uuid, %s::uuid, %s::jsonb)",
                    [str(review_id), expected_seq, type, str(actor_id),
                     str(stage_id) if stage_id else None, json.dumps(payload)],
                )
                return cur.fetchone()[0]
    except DatabaseError as exc:
        if "concurrency conflict" in str(exc):
            raise ConcurrencyError(str(exc)) from exc
        raise


def append(review_id, type, actor_id, payload, stage_id=None, retries=8):
    """Hängt ein Ereignis an und wiederholt bei gleichzeitigem Schreiben eines anderen Prozesses."""
    for _ in range(retries):
        with connection.cursor() as cur:
            cur.execute("SELECT last_seq FROM review WHERE review_id = %s::uuid", [str(review_id)])
            row = cur.fetchone()
        if row is None:
            raise ValueError(f"Review {review_id} existiert nicht.")
        try:
            return append_expected(review_id, row[0], type, actor_id, payload, stage_id)
        except ConcurrencyError:
            continue
    raise ConcurrencyError("Ereignis konnte nach mehreren Versuchen nicht angehängt werden.")


def verify_chain(review_id):
    with connection.cursor() as cur:
        cur.execute("SELECT n_events, n_bad, first_bad_seq FROM verify_chain(%s::uuid)", [str(review_id)])
        n_events, n_bad, first_bad = cur.fetchone()
    return {"n_events": n_events, "n_bad": n_bad, "first_bad_seq": first_bad, "ok": n_bad == 0}
